pub mod ark_matrix;
pub mod ark_stages;
pub mod cache;
pub mod fk_data;
pub mod formulas;
pub mod http;
pub mod item_names;
pub mod operator_data;
pub mod skill_data;
pub mod zones;

pub use cache::{BoxFuture, FetchError, Source};

/// 外部サイトから取得する情報をまとめて保持するレジストリ（bot にも api にも
/// 依存しない）。起動時に [`ExternalSourceRegistry::load`] で一括fetchし、以後は
/// 各情報源の [`Source`] がメモリに保持する値を機能側が参照する。
///
/// 新しい情報源を足す手順:
/// 1. `external_source/` 配下に新規モジュールを作り、対象データ型
///    （`Serialize + DeserializeOwned`）と
///    `pub fn fetch() -> BoxFuture<'static, Result<T, FetchError>>` を定義する
///    （中身は `operator_data.rs` を参照）。HTTP fetchは `http::client()` /
///    `http::fetch_json_with_retry()`（7sタイムアウト・最大10回リトライ、
///    全情報源共通）を使う。
/// 2. Seedで代替したいなら `pub const SEED_PATH: &str = "data/seed/xxx.json";`
///    と `pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>>`
///    （中身は `fetch()` を呼んで `cache::write_seed_file` に渡すだけ）を足し、
///    下の [`SEED_JOBS`] に1エントリ追加する（`regen_seeds` バイナリが使う）。
/// 3. このstructにフィールドを足し、[`ExternalSourceRegistry::load`] /
///    [`ExternalSourceRegistry::refresh_all`] / [`ExternalSourceRegistry::refresh_by_name`]
///    に1行ずつ追加する。
pub struct ExternalSourceRegistry {
    /// オペレーターの中国語→日本語名変換 + 昇進/スキル特化/モジュール消費素材データ
    /// （character_table.json / uniequip_table.json / char_patch_table.json を1回のfetchで
    /// まとめて構築する。旧`operator_names`はこれに統合済み）。
    pub operator_data: Source<operator_data::OperatorData>,
    /// スキルID→表示名+説明文（最大レベルの説明文をヘッダ込みで組み立て済み）。
    pub skill_data: Source<skill_data::SkillData>,
    pub item_names: Source<item_names::ItemNames>,
    pub zones: Source<zones::Zones>,
    pub ark_stages: Source<ark_stages::ArkStages>,
    pub ark_matrix: Source<ark_matrix::ArkMatrix>,
    pub formulas: Source<formulas::Formulas>,
    /// FK情報スプレッドシートの生データ。1時間毎の鮮度管理は
    /// `engine::fk_data_search::FkDataSearchEngine` が読み取り駆動で行うため、
    /// ここでの日次一括更新（[`refresh_all`](Self::refresh_all)）の対象には含めない。
    pub fk_data: Source<fk_data::FkSheetData>,
}

impl ExternalSourceRegistry {
    /// 起動時に全情報源を一度だけfetchする。個別のfetch失敗時の扱いは
    /// [`Source::load`] を参照（Seedがあればそれで代替、無ければpanic）。
    pub async fn load() -> Self {
        Self {
            operator_data: Source::load("operator_data", Some(operator_data::SEED_PATH), operator_data::fetch).await,
            skill_data: Source::load("skill_data", Some(skill_data::SEED_PATH), skill_data::fetch).await,
            item_names: Source::load("item_names", Some(item_names::SEED_PATH), item_names::fetch).await,
            zones: Source::load("zones", Some(zones::SEED_PATH), zones::fetch).await,
            ark_stages: Source::load("ark_stages", Some(ark_stages::SEED_PATH), ark_stages::fetch).await,
            ark_matrix: Source::load("ark_matrix", Some(ark_matrix::SEED_PATH), ark_matrix::fetch).await,
            formulas: Source::load("formulas", Some(formulas::SEED_PATH), formulas::fetch).await,
            fk_data: Source::load("fk_data", Some(fk_data::SEED_PATH), fk_data::fetch).await,
        }
    }

    /// 全情報源を再fetchする（1日1回程度の定期実行を想定）。ある情報源の
    /// fetchが失敗しても他の情報源には影響しない。
    pub async fn refresh_all(&self) {
        tokio::join!(
            self.operator_data.refresh(),
            self.skill_data.refresh(),
            self.item_names.refresh(),
            self.zones.refresh(),
            self.ark_stages.refresh(),
            self.ark_matrix.refresh(),
            self.formulas.refresh(),
        );
    }

    /// 名前を指定して1つだけ再fetchする（機能側からのオンデマンド更新用。
    /// risei_calculator_engine が自前のキャッシュ期限切れ時に ark_stages /
    /// ark_matrix を再fetchする用途を想定）。
    /// 該当する情報源が無ければ `None`。
    #[allow(dead_code)]
    pub async fn refresh_by_name(&self, name: &str) -> Option<bool> {
        match name {
            "operator_data" => Some(self.operator_data.refresh().await),
            "skill_data" => Some(self.skill_data.refresh().await),
            "item_names" => Some(self.item_names.refresh().await),
            "zones" => Some(self.zones.refresh().await),
            "ark_stages" => Some(self.ark_stages.refresh().await),
            "ark_matrix" => Some(self.ark_matrix.refresh().await),
            "formulas" => Some(self.formulas.refresh().await),
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
pub const SEED_JOBS: &[SeedJob] = &[
    SeedJob {
        name: "operator_data",
        path: operator_data::SEED_PATH,
        update: operator_data::update_seed,
    },
    SeedJob {
        name: "skill_data",
        path: skill_data::SEED_PATH,
        update: skill_data::update_seed,
    },
    SeedJob {
        name: "item_names",
        path: item_names::SEED_PATH,
        update: item_names::update_seed,
    },
    SeedJob {
        name: "zones",
        path: zones::SEED_PATH,
        update: zones::update_seed,
    },
    SeedJob {
        name: "ark_stages",
        path: ark_stages::SEED_PATH,
        update: ark_stages::update_seed,
    },
    SeedJob {
        name: "ark_matrix",
        path: ark_matrix::SEED_PATH,
        update: ark_matrix::update_seed,
    },
    SeedJob {
        name: "formulas",
        path: formulas::SEED_PATH,
        update: formulas::update_seed,
    },
    SeedJob {
        name: "fk_data",
        path: fk_data::SEED_PATH,
        update: fk_data::update_seed,
    },
];
