pub mod calc;
pub mod format;
pub mod matcher;
pub mod model;

/// recruit ドメイン共通のエラー型（bot にも api にも依存しない）
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// 求人ドメインの全データ（辞書＋オペレーターDB）をまとめて保持。
/// 起動時に一度ロードして bot / api で共有する。
pub struct RecruitEngine {
    pub matcher: matcher::Matcher,
    pub data: calc::RecruitData,
}

impl RecruitEngine {
    pub fn load() -> Result<Self, Error> {
        Ok(Self {
            matcher: matcher::Matcher::load()?,
            data: calc::RecruitData::load()?,
        })
    }
}
