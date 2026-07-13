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

    /// Web API の doRecruitment と完全一致する処理。
    /// OCR生テキスト → タグ抽出 → 計算 → title/reply(responseForAI) 文字列。
    pub fn process_from_ocr(&self, ocr_text: &str, pickup: Option<&[String]>) -> TagReply {
        let matched = self.matcher.match_tag(ocr_text);

        // isEmpty チェック（Python: matchTag.isEmpty()）
        if matched.matches.is_empty() {
            return TagReply {
                title: "エラー".to_string(),
                reply: "タグがありません".to_string(),
            };
        }

        // matches を Vec 化。8個超なら先頭8個に切り詰め（Python の list(matches)[:8]）
        // 注意: 8個超は OCR 大誤爆時のみ。順序は Python set と一致しない（許容）。
        let mut matches: Vec<String> = matched.matches.into_iter().collect();
        if matches.len() > 8 {
            matches.truncate(8);
        }

        let is_global = matched.is_global;
        let results = self.data.calculate(&matches, is_global, 4, pickup);

        let sorted_input = self.data.normalize_names(&matches);
        let title = format::make_title(&sorted_input, is_global, true);

        let reply = if results.is_empty() {
            "★4以上になる組み合わせはありません".to_string()
        } else {
            format::response_for_ai(results)
        };

        TagReply { title, reply }
    }

    /// bot 用：title と embbedContents（chunks）を返す。
    /// Python bot の embed 表示に対応。
    pub fn process_for_embed(&self, ocr_text: &str) -> EmbedReply {
        let matched = self.matcher.match_tag(ocr_text);
        if matched.matches.is_empty() {
            return EmbedReply {
                title: "エラー".to_string(),
                chunks: vec!["タグがありません".to_string()],
                msg_type: MsgType::Err,
            };
        }
        let mut matches: Vec<String> = matched.matches.into_iter().collect();
        if matches.len() > 8 {
            matches.truncate(8);
        }
        let is_global = matched.is_global;
        let results = self.data.calculate(&matches, is_global, 4, None);
        let sorted_input = self.data.normalize_names(&matches);
        let title = format::make_title(&sorted_input, is_global, true);
        let chunks = if results.is_empty() {
            vec!["★4以上になる組み合わせはありません".to_string()]
        } else {
            format::display_chunks(results)
        };
        EmbedReply {
            title,
            chunks,
            msg_type: MsgType::Ok,
        }
    }
}

/// Python の TagReplyData に対応（API 用）。
pub struct TagReply {
    pub title: String,
    pub reply: String,
}

/// bot の embed 出力用。
pub struct EmbedReply {
    pub title: String,
    pub chunks: Vec<String>,
    pub msg_type: MsgType,
}

impl EmbedReply {
    /// エラー表示用のコンストラクタ（Python の actionToDiscord のエラー系に対応）。
    pub fn error(msg: &str) -> EmbedReply {
        EmbedReply {
            title: "エラー".to_string(),
            chunks: vec![msg.to_string()],
            msg_type: MsgType::Err,
        }
    }
}

/// メッセージ種別（Python の RCMsgType）
#[derive(Clone, Copy)]
pub enum MsgType {
    Ok,
    Err,
}

impl MsgType {
    /// Python の colour() に対応。0x8be02b(緑) / マゼンタ。
    pub fn colour(&self) -> u32 {
        match self {
            MsgType::Ok => 0x8be02b,
            MsgType::Err => 0xff00ff, // magenta
        }
    }
}
