pub mod ark_matrix;
pub mod ark_stages;
pub mod cache;
pub mod fk_data;
pub mod formulas;
pub mod http;
pub mod item_names;
pub mod jp_first_seen;
pub mod operator_birthdays;
pub mod operator_combat;
pub mod operator_data;
pub mod recruit_pool;
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
///    [`ExternalSourceRegistry::refresh_all`] に1行ずつ追加する。
pub struct ExternalSourceRegistry {
    /// オペレーターの中国語→日本語名変換 + 昇進/スキル特化/モジュール消費素材データ
    /// （character_table.json / uniequip_table.json / char_patch_table.json を1回のfetchで
    /// まとめて構築する。旧`operator_names`はこれに統合済み）。
    pub operator_data: Source<operator_data::OperatorData>,
    /// フレームキル計算機用のオペレーター戦闘生データ（元ATK/潜在ATK/モジュールATK加算値）。
    /// `operator_data`とは意図的に別ソース（消費素材ドメインと戦闘ドメインを分離するため）。
    pub operator_combat: Source<operator_combat::OperatorCombat>,
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
    /// 公開求人のプール（JPの募集可能一覧 + CNで先に追加された分）。日次更新後に
    /// `RecruitEngine::rebuild`で求人計算のタグ→オペレーター表を作り直す（main.rs）。
    pub recruit_pool: Source<recruit_pool::RecruitPool>,
    /// オペレーターの誕生日（bot の誕生日お祝い用。プロフィールの「生日」から作る）。
    pub operator_birthdays: Source<operator_birthdays::OperatorBirthdays>,
}

impl ExternalSourceRegistry {
    /// 起動時に全情報源を一度だけfetchする。個別のfetch失敗時の扱いは
    /// [`Source::load`] を参照（Seedがあればそれで代替、無ければpanic）。
    ///
    /// `debug`がtrueなら（`--debug`起動時）fetchを一切行わず、常にSeedを使う。
    /// 以後の再fetch（[`refresh_all`](Self::refresh_all)や
    /// 各機能側のTTL駆動更新）も同様にno-opになる（[`Source::refresh`]参照）。
    pub async fn load(debug: bool) -> Self {
        Self {
            operator_data: Source::load("operator_data", Some(operator_data::SEED_PATH), operator_data::fetch, debug).await,
            operator_combat: Source::load("operator_combat", Some(operator_combat::SEED_PATH), operator_combat::fetch, debug).await,
            skill_data: Source::load("skill_data", Some(skill_data::SEED_PATH), skill_data::fetch, debug).await,
            item_names: Source::load("item_names", Some(item_names::SEED_PATH), item_names::fetch, debug).await,
            zones: Source::load("zones", Some(zones::SEED_PATH), zones::fetch, debug).await,
            ark_stages: Source::load("ark_stages", Some(ark_stages::SEED_PATH), ark_stages::fetch, debug).await,
            ark_matrix: Source::load("ark_matrix", Some(ark_matrix::SEED_PATH), ark_matrix::fetch, debug).await,
            formulas: Source::load("formulas", Some(formulas::SEED_PATH), formulas::fetch, debug).await,
            fk_data: Source::load("fk_data", Some(fk_data::SEED_PATH), fk_data::fetch, debug).await,
            recruit_pool: Source::load("recruit_pool", Some(recruit_pool::SEED_PATH), recruit_pool::fetch, debug).await,
            operator_birthdays: Source::load("operator_birthdays", Some(operator_birthdays::SEED_PATH), operator_birthdays::fetch, debug).await,
        }
    }

    /// 全情報源を再fetchする（1日1回程度の定期実行を想定）。ある情報源の
    /// fetchが失敗しても他の情報源には影響しない。
    pub async fn refresh_all(&self) {
        tokio::join!(
            self.operator_data.refresh(),
            self.operator_combat.refresh(),
            self.skill_data.refresh(),
            self.item_names.refresh(),
            self.zones.refresh(),
            //self.ark_stages.refresh(), //RiseiCalculatorEngine::refreshが2hごとに再fetchするため、デイリー更新対象から外す
            //self.ark_matrix.refresh(), //RiseiCalculatorEngine::refreshが2hごとに再fetchするため、デイリー更新対象から外す
            self.formulas.refresh(),
            self.recruit_pool.refresh(),
            self.operator_birthdays.refresh(),
        );
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
        name: "operator_combat",
        path: operator_combat::SEED_PATH,
        update: operator_combat::update_seed,
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
    SeedJob {
        name: "recruit_pool",
        path: recruit_pool::SEED_PATH,
        update: recruit_pool::update_seed,
    },
    SeedJob {
        name: "operator_birthdays",
        path: operator_birthdays::SEED_PATH,
        update: operator_birthdays::update_seed,
    },
    // Source ではない（実行時はfetchしない）。JPに初めて現れた日の記録を差分だけ追記する。
    SeedJob {
        name: "jp_first_seen",
        path: jp_first_seen::SEED_PATH,
        update: jp_first_seen::update_seed,
    },
];
