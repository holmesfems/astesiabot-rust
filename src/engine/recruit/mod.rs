pub mod calc;
pub mod edit;
pub mod format;
pub mod jp_release;
pub mod matcher;
pub mod model;

use crate::engine::external_source::recruit_pool::RecruitPool;
use std::sync::{Arc, RwLock};

/// recruit ドメイン共通のエラー型（bot にも api にも依存しない）
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// 求人ドメインの全データ（辞書＋公開求人のプールから組み立てた計算用データ）をまとめて保持。
/// 起動時に一度ロードして bot / api で共有する。
pub struct RecruitEngine {
    pub matcher: matcher::Matcher,
    /// プールの日次更新で丸ごと差し替える（[`RecruitEngine::rebuild`]）。読む側は
    /// [`RecruitEngine::data`] でその時点のスナップショット（Arc）を受け取る。
    data: RwLock<Arc<calc::RecruitData>>,
}

impl RecruitEngine {
    pub fn load(pool: &RecruitPool) -> Result<Self, Error> {
        Ok(Self {
            matcher: matcher::Matcher::load()?,
            data: RwLock::new(Arc::new(calc::RecruitData::build(pool)?)),
        })
    }

    /// 現在の計算用データ。awaitを跨いで持っていてよい（更新は差し替えなので影響しない）。
    pub fn data(&self) -> Arc<calc::RecruitData> {
        self.data.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 新しいプールで計算用データを作り直して差し替える。失敗したら何もしない
    /// （直前のデータのまま）。
    pub fn rebuild(&self, pool: &RecruitPool) -> Result<(), Error> {
        let data = Arc::new(calc::RecruitData::build(pool)?);
        *self.data.write().unwrap_or_else(|e| e.into_inner()) = data;
        Ok(())
    }

    /// テスト用: Seed（data/seed/recruit_pool.json）のプールでロードする。
    #[cfg(test)]
    pub(crate) fn load_from_seed() -> Self {
        use crate::engine::external_source::recruit_pool::SEED_PATH;
        let json = std::fs::read_to_string(SEED_PATH)
            .unwrap_or_else(|e| panic!("seed({SEED_PATH})の読み込みに失敗: {e}。先に`cargo run --bin regen_seeds`を実行すること"));
        let pool: RecruitPool = serde_json::from_str(&json).expect("seed should parse as RecruitPool");
        Self::load(&pool).expect("recruit data should load")
    }
}
