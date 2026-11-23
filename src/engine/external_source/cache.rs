use serde::{de::DeserializeOwned, Serialize};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type FetchError = Box<dyn std::error::Error + Send + Sync>;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 各情報源が実装するfetch関数の型。キャプチャの無い関数ポインタなので、
/// クロージャではなく `pub fn fetch() -> BoxFuture<'static, Result<T, FetchError>>`
/// という形のトップレベル関数として定義する（operator_data.rs を参照）。
pub type FetchFn<T> = fn() -> BoxFuture<'static, Result<T, FetchError>>;

/// 外部サイトから取得する情報1つ分のキャッシュ。
///
/// - 起動時は [`Source::load`] で最初の取得を行う。失敗した場合、Seed
///   （`seed_path` に指定したJSONファイル）があればそれで代替し、無ければ
///   panicする。
/// - 起動後は [`Source::refresh`] を呼ぶたびに再fetchする。失敗した場合は
///   何もせず、直前に保持していたメモリをそのまま使い続ける。
///
/// Seedは実行時には**書き込まない**（Heroku等ではファイル書き込みが
/// dyno再起動で揮発するため、実行時に書いても意味が無い）。Seedファイルは
/// `cargo run --bin regen_seeds` で手動生成し、git commit / push して
/// リポジトリに含めておく運用にする（各ソースモジュールの `update_seed()` /
/// [`write_seed_file`] を参照）。
pub struct Source<T> {
    name: &'static str,
    fetch_fn: FetchFn<T>,
    cache: RwLock<Arc<T>>,
    /// `--debug`起動時、常にSeedのままにするためのフラグ。
    force_seed_only: bool,
}

impl<T: Serialize + DeserializeOwned + Send + Sync + 'static> Source<T> {
    /// 起動時の初回ロード。`force_seed_only`は`--debug`起動用。
    pub async fn load(
        name: &'static str,
        seed_path: Option<&'static str>,
        fetch_fn: FetchFn<T>,
        force_seed_only: bool,
    ) -> Self {
        let data = if force_seed_only {
            let path = seed_path.unwrap_or_else(|| {
                panic!("[outer_source:{name}] --debugはSeedが必須ですが、このソースにはSeedがありません")
            });
            eprintln!("[outer_source:{name}] --debug: Seed({path})を使用します（fetchはしません）");
            Self::read_seed(path).unwrap_or_else(|| {
                panic!("[outer_source:{name}] --debug: Seed({path})の読み込みに失敗しました")
            })
        } else {
            match fetch_fn().await {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[outer_source:{name}] 起動時fetchに失敗しました: {e}");
                    match seed_path.and_then(Self::read_seed) {
                        Some(seed) => {
                            let path = seed_path.expect("seed_path is Some here");
                            eprintln!("[outer_source:{name}] Seed({path})で代替します");
                            seed
                        }
                        None => panic!(
                            "[outer_source:{name}] fetchに失敗し、Seedも無いため起動できません: {e}"
                        ),
                    }
                }
            }
        };
        Self {
            name,
            fetch_fn,
            cache: RwLock::new(Arc::new(data)),
            force_seed_only,
        }
    }

    pub async fn get(&self) -> Arc<T> {
        self.cache.read().await.clone()
    }

    /// 再fetchしてメモリを更新し、成否を返す（ログ表示用）。
    pub async fn refresh(&self) -> bool {
        if self.force_seed_only {
            return false;
        }
        match (self.fetch_fn)().await {
            Ok(v) => {
                *self.cache.write().await = Arc::new(v);
                true
            }
            Err(e) => {
                eprintln!(
                    "[outer_source:{}] 再fetchに失敗、既存メモリを保持します: {e}",
                    self.name
                );
                false
            }
        }
    }

    fn read_seed(path: &str) -> Option<T> {
        let s = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&s).ok()
    }
}

/// SeedのJSONファイルを書き出す。実行時の `Source` からは呼ばない
/// （`regen_seeds` 専用）。元データ（HashMap等）の反復順はプロセス毎に不定なので、
/// キーをソートしてgit diffが毎回出ないようにする。
pub fn write_seed_file<T: Serialize>(path: &str, data: &T) -> Result<(), FetchError> {
    let value = serde_json::to_value(data)?;
    let sorted = sort_json_keys(value);
    let json = serde_json::to_string_pretty(&sorted)?;
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)?;
    Ok(())
}

fn sort_json_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(String, serde_json::Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let sorted: serde_json::Map<String, serde_json::Value> = entries
                .into_iter()
                .map(|(k, v)| (k, sort_json_keys(v)))
                .collect();
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sort_json_keys).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    static REFRESH_SHOULD_FAIL: AtomicBool = AtomicBool::new(false);
    fn fetch_for_refresh_test() -> BoxFuture<'static, Result<u32, FetchError>> {
        Box::pin(async {
            if REFRESH_SHOULD_FAIL.load(Ordering::SeqCst) {
                Err("boom".into())
            } else {
                Ok(42)
            }
        })
    }

    #[tokio::test]
    async fn refresh_keeps_previous_value_on_failure() {
        REFRESH_SHOULD_FAIL.store(false, Ordering::SeqCst);
        let source = Source::load("test_refresh_keep", None, fetch_for_refresh_test, false).await;
        assert_eq!(*source.get().await, 42);

        REFRESH_SHOULD_FAIL.store(true, Ordering::SeqCst);
        assert!(!source.refresh().await, "refresh should report failure");
        assert_eq!(
            *source.get().await,
            42,
            "value from before the failed refresh must be kept"
        );
    }

    fn fetch_always_fails() -> BoxFuture<'static, Result<u32, FetchError>> {
        Box::pin(async { Err("always fails".into()) })
    }

    #[tokio::test]
    #[should_panic(expected = "Seedも無いため起動できません")]
    async fn load_panics_when_fetch_fails_and_no_seed() {
        let _: Source<u32> = Source::load("test_panic", None, fetch_always_fails, false).await;
    }

    #[tokio::test]
    async fn load_falls_back_to_seed_when_fetch_fails() {
        let path: &'static str = Box::leak(
            std::env::temp_dir()
                .join("astesiabot_test_seed_fallback.json")
                .to_string_lossy()
                .into_owned()
                .into_boxed_str(),
        );
        std::fs::write(path, "99").unwrap();

        let source: Source<u32> =
            Source::load("test_seed", Some(path), fetch_always_fails, false).await;
        assert_eq!(*source.get().await, 99);

        std::fs::remove_file(path).ok();
    }

    static SEED_ONLY_FETCH_CALLED: AtomicBool = AtomicBool::new(false);
    fn fetch_flags_call() -> BoxFuture<'static, Result<u32, FetchError>> {
        Box::pin(async {
            SEED_ONLY_FETCH_CALLED.store(true, Ordering::SeqCst);
            Ok(1)
        })
    }

    #[tokio::test]
    async fn force_seed_only_reads_seed_and_never_fetches() {
        let path: &'static str = Box::leak(
            std::env::temp_dir()
                .join("astesiabot_test_seed_only.json")
                .to_string_lossy()
                .into_owned()
                .into_boxed_str(),
        );
        std::fs::write(path, "77").unwrap();
        SEED_ONLY_FETCH_CALLED.store(false, Ordering::SeqCst);

        let source: Source<u32> =
            Source::load("test_seed_only", Some(path), fetch_flags_call, true).await;
        assert_eq!(*source.get().await, 77);
        assert!(!source.refresh().await, "refresh must report no update");
        assert_eq!(*source.get().await, 77);
        assert!(
            !SEED_ONLY_FETCH_CALLED.load(Ordering::SeqCst),
            "fetch must not be called in seed-only mode"
        );

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn write_seed_file_sorts_keys_recursively_and_keeps_array_order() {
        let dir =
            std::env::temp_dir().join(format!("astesiabot_test_write_seed_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("nested").join("seed.json");
        let data = serde_json::json!({
            "b": 1,
            "a": { "z": 1, "y": 2 },
            "list": [ { "d": 1, "c": 2 }, 3, [ { "q": 1, "p": 2 } ] ],
            "arr_order": [3, 1, 2],
        });

        write_seed_file(path.to_str().unwrap(), &data).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value, data, "content must be unchanged");
        let pos = |needle: &str| text.find(needle).unwrap();
        assert!(pos("\"a\"") < pos("\"arr_order\""));
        assert!(pos("\"arr_order\"") < pos("\"b\""));
        assert!(pos("\"b\"") < pos("\"list\""));
        assert!(pos("\"y\"") < pos("\"z\""), "nested object keys sorted");
        assert!(pos("\"c\"") < pos("\"d\""), "keys inside arrays sorted");
        assert!(
            pos("\"p\"") < pos("\"q\""),
            "keys inside nested arrays sorted"
        );
        assert_eq!(value["arr_order"], serde_json::json!([3, 1, 2]));
    }
}
