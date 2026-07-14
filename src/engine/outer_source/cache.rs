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
/// という形のトップレベル関数として定義する（operator_names.rs を参照）。
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
}

impl<T: Serialize + DeserializeOwned + Send + Sync + 'static> Source<T> {
    /// 起動時の初回ロード。fetch失敗時はSeed参照、Seedも無ければpanicする。
    pub async fn load(name: &'static str, seed_path: Option<&'static str>, fetch_fn: FetchFn<T>) -> Self {
        let data = match fetch_fn().await {
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
        };
        Self {
            name,
            fetch_fn,
            cache: RwLock::new(Arc::new(data)),
        }
    }

    /// 現在メモリに保持している情報のスナップショットを取得する。
    pub async fn get(&self) -> Arc<T> {
        self.cache.read().await.clone()
    }

    /// 再fetchしてメモリを更新する。失敗した場合は直前のメモリを保持し続ける。
    /// 戻り値は成否（呼び出し元がログ表示等に使えるように）。
    pub async fn refresh(&self) -> bool {
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

/// SeedのJSONファイルを書き出す（親ディレクトリが無ければ作成する）。
/// 実行時の `Source` からは呼ばない。各ソースモジュールの `update_seed()`
/// から、Seed手動更新ツール（`cargo run --bin regen_seeds`）実行時にのみ使う。
pub fn write_seed_file<T: Serialize>(path: &str, data: &T) -> Result<(), FetchError> {
    let json = serde_json::to_string_pretty(data)?;
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)?;
    Ok(())
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
        let source = Source::load("test_refresh_keep", None, fetch_for_refresh_test).await;
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
        let _: Source<u32> = Source::load("test_panic", None, fetch_always_fails).await;
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

        let source: Source<u32> = Source::load("test_seed", Some(path), fetch_always_fails).await;
        assert_eq!(*source.get().await, 99);

        std::fs::remove_file(path).ok();
    }
}
