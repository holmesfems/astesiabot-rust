use indexmap::IndexMap;

/// FK情報の検索結果（4分岐）。
pub enum FkSearchResult {
    /// 指定のオペレーターのFK情報が見つからない。
    OperatorNotFound,
    /// スキル未指定かつ複数のFKスキルを持つため、選択を促す必要がある。
    /// skillNum -> 表示名（解決できなければskillNumそのもの）。
    NeedsSkillSelection { choices: IndexMap<String, String> },
    /// 指定されたスキルがFK情報として見つからない。候補一覧を提示する。
    SkillNotFound { candidates: Vec<SkillCandidate> },
    /// 解決成功。
    Found(FkSkillView),
}

pub struct SkillCandidate {
    pub skill_num: String,
    pub skill_name: String,
}

pub struct FkSkillView {
    /// 解決できたスキル名。解決できなければ空文字列。
    pub skill_name: String,
    /// ユーザーが入力したスキル指定の生値。`skill_name`が空の場合の表示フォールバックに使う
    /// （解決後のスキル番号ではなく、入力そのままを使う）。
    pub requested_skill_num: String,
    pub fk_num: String,
    pub fk_err: String,
    pub detail: String,
}
