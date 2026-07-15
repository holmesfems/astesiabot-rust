use super::Error;
use fancy_regex::Regex;
use std::collections::{BTreeMap, HashSet};

/// マッチング結果。matches とグローバル版フラグ。
pub struct MatchResult {
    pub matches: HashSet<String>,
    pub is_global: bool,
}

/// 3言語の辞書とextra辞書を保持。起動時に一度読み込む。
pub struct Matcher {
    ja: BTreeMap<String, String>,
    en: BTreeMap<String, String>,
    zh: BTreeMap<String, String>,
    ja_extra: Vec<(Regex, String)>,
    zh_extra: Vec<(Regex, String)>,
    replace: Vec<(Regex, String)>,
    clear: Regex,
}

impl Matcher {
    pub fn load() -> Result<Self, Error> {
        let ja: BTreeMap<String, String> =
            serde_yaml::from_str(&std::fs::read_to_string("data/recruitment/tagJaToJa.yaml")?)?;
        let en: BTreeMap<String, String> =
            serde_yaml::from_str(&std::fs::read_to_string("data/recruitment/tagEnToJa.yaml")?)?;
        let zh: BTreeMap<String, String> =
            serde_yaml::from_str(&std::fs::read_to_string("data/recruitment/tagZhToJa.yaml")?)?;

        // ja_extra（Python の __jaExtraDict）。順序が意味を持つので Vec で保持。
        let ja_extra = compile_pairs(&[
            (r"範[围圍囲困匯田]攻[擊撃]", "範囲攻撃"),
            (r"(?!上級)(..?)?ー下", "エリート"),
            (r"医療...?", "医療"),
            (r"上級..ー下", "上級エリート"),
            (r"補助...?", "補助"),
            (r"狙撃...?", "狙撃"),
            (r"前衛...?", "前衛"),
            (r"COST(O)?", "COST回復"),
            (r"防御.", "防御"),
            (r"重装...?", "重装"),
            (r"上級エリード", "上級エリート"),
            (r"エリード", "エリート"),
            (r"特殊..?.?", "特殊"),
            (r"[35]{2}1t", "弱化"),
            (r"、弁】", "爆発力"),
            (r"[匠近]距[离離]", "近距離"),
            (r"[攴支][扶援]", "支援"),
            (r"[術市]師...?", "術師"),
        ])?;

        let zh_extra = compile_pairs(&[(r"費用回复", "COST回復"), (r"治疔", "治療")])?;

        // replacedict（誤字修正）
        let replace = compile_pairs(&[
            (r"工", "エ"),
            (r"夕", "タ"),
            (r"一", "ー"),
            (r"卜", "ト"),
            (r"り", "リ"),
            (r"ブ", "プ"),
            (r"カ", "力"),
            (r"口", "ロ"),
            (r"エ[丿ノ刂]ート", "エリート"),
            (r"[于千]员", "干员"),
        ])?;

        let clear = Regex::new(r"[．•་.,·・´`‧˙。¸Ⓡ【®:「、]+|^[-]+|[-]+$")?;

        Ok(Self {
            ja,
            en,
            zh,
            ja_extra,
            zh_extra,
            replace,
            clear,
        })
    }

    /// OCR全文テキストからタグ集合を判定（Python の matchTag）。
    pub fn match_tag(&self, result: &str) -> MatchResult {
        // 改行で分割 → strip → clear正規表現除去 → 誤字置換
        let mut lines: Vec<String> = result
            .split('\n')
            .map(|s| s.trim().to_string())
            .map(|s| self.clear.replace_all(&s, "").to_string())
            .collect();
        for (re, rep) in &self.replace {
            lines = lines
                .iter()
                .map(|l| re.replace_all(l, rep.as_str()).to_string())
                .collect();
        }

        let jp = self.match_core(&lines, &self.ja, Some(&self.ja_extra));
        if jp.len() >= 5 {
            return MatchResult {
                matches: jp,
                is_global: true,
            };
        }
        let en = self.match_core(&lines, &self.en, None);
        if en.len() >= 5 {
            return MatchResult {
                matches: en,
                is_global: true,
            };
        }
        let zh = self.match_core(&lines, &self.zh, Some(&self.zh_extra));
        if zh.len() >= 5 {
            return MatchResult {
                matches: zh,
                is_global: false,
            };
        }
        // フォールバック：日中結合
        let jpzh: HashSet<String> = jp.union(&zh).cloned().collect();
        if jpzh.len() >= en.len() {
            return MatchResult {
                matches: jpzh,
                is_global: jp.len() >= zh.len(),
            };
        }
        MatchResult {
            matches: en,
            is_global: true,
        }
    }

    /// Python の matchTagCoreProcess。完全一致 or 空白split一致 → extraで正規表現補正。
    fn match_core(
        &self,
        result: &[String],
        base: &BTreeMap<String, String>,
        extra: Option<&[(Regex, String)]>,
    ) -> HashSet<String> {
        let mut ret = HashSet::new();
        for (key, value) in base {
            if ret.contains(value) {
                continue;
            }
            let hit = result
                .iter()
                .any(|text| key == text || text.split(' ').any(|part| part == key));
            if hit {
                ret.insert(value.clone());
            }
        }
        let extra = match extra {
            Some(e) if ret.len() != 5 => e,
            _ => return ret,
        };
        for (re, value) in extra {
            if ret.len() >= 5 {
                break;
            }
            if ret.contains(value) {
                continue;
            }
            let hit = result.iter().any(|text| {
                matches_at_start(re, text)
                    || text.split(' ').any(|part| matches_at_start(re, part))
            });
            if hit {
                ret.insert(value.clone());
            }
        }
        ret
    }
}

/// Python の re.match（先頭一致）相当。fancy-regex では find の結果が
/// 位置0から始まるかで判定する。エラー時は false 扱い。
fn matches_at_start(re: &Regex, text: &str) -> bool {
    matches!(re.find(text), Ok(Some(m)) if m.start() == 0)
}

/// (パターン文字列, 置換先) の配列をコンパイル済みペアに変換。
fn compile_pairs(pairs: &[(&str, &str)]) -> Result<Vec<(Regex, String)>, Error> {
    let mut out = Vec::new();
    for (pat, rep) in pairs {
        out.push((Regex::new(pat)?, rep.to_string()));
    }
    Ok(out)
}
