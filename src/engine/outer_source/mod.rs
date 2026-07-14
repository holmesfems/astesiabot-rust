pub mod cache;
pub mod http;
pub mod operator_names;

pub use cache::{BoxFuture, FetchError, Source};

/// 外部サイトから取得する情報をまとめて保持するレジストリ（bot にも api にも
/// 依存しない）。起動時に [`OuterSourceRegistry::load`] で一括fetchし、以後は
/// 各情報源の [`Source`] がメモリに保持する値を機能側が参照する。
///
/// 新しい情報源を足す手順:
/// 1. `outer_source/` 配下に新規モジュールを作り、対象データ型
///    （`Serialize + DeserializeOwned`）と
///    `pub fn fetch() -> BoxFuture<'static, Result<T, FetchError>>` を定義する
///    （中身は `operator_names.rs` を参照）。HTTP fetchは `http::client()` /
///    `http::fetch_json_with_retry()`（7sタイムアウト・最大10回リトライ、
///    全情報源共通）を使う。
/// 2. Seedで代替したいなら `pub const SEED_PATH: &str = "data/seed/xxx.json";`
///    と `pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>>`
///    （中身は `fetch()` を呼んで `cache::write_seed_file` に渡すだけ）を足し、
///    下の [`SEED_JOBS`] に1エントリ追加する（`regen_seeds` バイナリが使う）。
/// 3. このstructにフィールドを足し、[`OuterSourceRegistry::load`] /
///    [`OuterSourceRegistry::refresh_all`] / [`OuterSourceRegistry::refresh_by_name`]
///    に1行ずつ追加する。
pub struct OuterSourceRegistry {
    pub operator_names: Source<operator_names::OperatorNames>,
}

impl OuterSourceRegistry {
    /// 起動時に全情報源を一度だけfetchする。個別のfetch失敗時の扱いは
    /// [`Source::load`] を参照（Seedがあればそれで代替、無ければpanic）。
    pub async fn load() -> Self {
        Self {
            operator_names: Source::load(
                "operator_names",
                Some(operator_names::SEED_PATH),
                operator_names::fetch,
            )
            .await,
        }
    }

    /// 全情報源を再fetchする（1日1回程度の定期実行を想定）。ある情報源の
    /// fetchが失敗しても他の情報源には影響しない。
    pub async fn refresh_all(&self) {
        tokio::join!(self.operator_names.refresh());
    }

    /// 名前を指定して1つだけ再fetchする（機能側からのオンデマンド更新用）。
    /// 該当する情報源が無ければ `None`。
    #[allow(dead_code)]
    pub async fn refresh_by_name(&self, name: &str) -> Option<bool> {
        match name {
            "operator_names" => Some(self.operator_names.refresh().await),
            _ => None,
        }
    }
}

/// Seedを手動生成するジョブ1件（名前・保存先・実行方法）。
pub struct SeedJob {
    pub name: &'static str,
    pub path: &'static str,
    pub update: fn() -> BoxFuture<'static, Result<(), FetchError>>,
}

/// Seedを持つ情報源の一覧。`cargo run --bin regen_seeds` がこれを順に実行する。
/// 新しい情報源にSeedを持たせたら、ここに1エントリ追加すること。
pub const SEED_JOBS: &[SeedJob] = &[SeedJob {
    name: "operator_names",
    path: operator_names::SEED_PATH,
    update: operator_names::update_seed,
}];
