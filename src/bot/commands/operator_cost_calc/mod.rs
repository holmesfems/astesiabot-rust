pub mod operatorcostlist;
pub mod operatorelitecost;
pub mod operatormastercost;
pub mod operatormodulecost;

use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply};
use crate::engine::operator_cost_calc::model::build_formula_map;
use crate::engine::operator_cost_calc::{AllOperatorsInfo, ValueSet};
use crate::engine::risei_calculator_engine::Server;

/// charmaterials.py側の`EPSILON = 1e-6`。個数表示の整数/小数丸め判定に使う
/// （`engine::operator_cost_calc::EPSILON`と同一値だが、こちらは表示整形専用として
/// コマンド層に置く。理性価値の閾値判定は既にengine層のランキング関数側で適用済み）。
const DISPLAY_EPSILON: f64 = 1e-6;

/// 4コマンド共通の計算コンテキストを構築する（Python版`OperatorCostsCalculator.operatorInfo`
/// + `CalculatorManager.getValues`相当）。`FormulaMap`はコマンド呼び出しごとに
/// `outer_source.formulas`のスナップショットから再構築する。
pub async fn build_context(state: &AppState) -> (AllOperatorsInfo, ValueSet) {
    let data = state.outer_source.operator_data.get().await;
    let item_names = state.outer_source.item_names.get().await;
    let skill_data = state.outer_source.skill_data.get().await;
    let formulas_raw = state.outer_source.formulas.get().await;
    let formulas = build_formula_map(&formulas_raw.formulas, &item_names);
    let info = AllOperatorsInfo {
        data,
        item_names,
        skill_data,
        formulas,
    };

    let global_snapshot = state.risei_calculator.snapshot(Server::Global, &state.outer_source).await;
    let mainland_snapshot = state.risei_calculator.snapshot(Server::Mainland, &state.outer_source).await;
    let values = ValueSet {
        global: global_snapshot.values.clone(),
        mainland: mainland_snapshot.values.clone(),
    };
    (info, values)
}

/// EmbedReply をスラッシュコマンドの応答として送信する（`bot/commands/risei/mod.rs`の
/// `send_reply`と同じ役割。feature間の結合を避けるためここに複製している）。
pub async fn send_reply(ctx: Context<'_>, reply: EmbedReply) -> Result<(), Error> {
    for batch in to_embed_batches(&reply) {
        let mut created = poise::CreateReply::default();
        created.embeds = batch;
        ctx.send(created).await?;
    }
    Ok(())
}

/// Python `dumpToPrint`(header無し版)。```で囲んだコードブロックにする。
pub fn dump_to_print(lines: &[String]) -> String {
    format!("```\n{}```", lines.join("\n"))
}

/// Python `"{0} × {1:d}".format(...)`/`"{0} × {1:.3f}".format(...)`の丸め判定込み表示。
fn fmt_item_line(name: &str, count: f64) -> String {
    let rounded = count.round();
    if (count - rounded).abs() < DISPLAY_EPSILON {
        format!("{name} × {}", rounded as i64)
    } else {
        format!("{name} × {count:.3}")
    }
}

/// Python `ItemCost.toStrBlock(sortByCount)`。`sort_by_count`時は個数降順に安定並べ替え
/// （タイは元の`value_target`順を保つ）。
pub fn fmt_item_block(items: &[(String, f64)], sort_by_count: bool) -> String {
    let mut items = items.to_vec();
    if sort_by_count {
        items.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    let lines: Vec<String> = items.iter().map(|(name, count)| fmt_item_line(name, *count)).collect();
    dump_to_print(&lines)
}

#[cfg(test)]
mod golden_tests {
    //! Python版(正解)の出力をゴールデンJSON化した`data/golden/operator_cost_calc/*.json`との
    //! 突き合わせ（`ref_python/RiseiCalculatorBot-main/dump_charmaterials_golden.py`で生成）。
    //! 実ネットワークに依存するため`#[ignore]`。`cargo test -- --ignored`で明示実行する。
    //! ゲームデータ更新で正当に変わるのは`list_*`系(ランキング/統計)のみ。ゲームデータ更新後は
    //! dumpスクリプトを再実行してゴールデンを更新し、`cargo run --bin regen_seeds`もセットで行うこと。
    use super::operatorcostlist::{cost_list_reply, CostListSelection};
    use super::operatorelitecost::elite_cost_reply;
    use super::operatormastercost::master_cost_reply;
    use super::operatormodulecost::module_cost_reply;
    use crate::api::AppState;
    use crate::bot::reply::EmbedReply;
    use crate::bot::services::moderation::ModerationState;
    use crate::engine;
    use fancy_regex::Regex;
    use serde::Deserialize;
    use std::sync::OnceLock;

    #[derive(Deserialize)]
    struct Golden {
        title: String,
        contents: Vec<String>,
    }

    fn load_golden(name: &str) -> Golden {
        let path = format!("data/golden/operator_cost_calc/{name}.json");
        let s = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
        serde_json::from_str(&s).unwrap_or_else(|e| panic!("failed to parse {path}: {e}"))
    }

    /// 理性価値表(risei_calculator_engine)は基準マップの初期選択に乱数を使うため
    /// （`Calculator::build`の`generateCategorySeed`。Python版も`random.choice`で同様）、
    /// 近接タイの複数カテゴリが実行ごとに異なる基準ステージへ収束し得る。これにより理性価値の
    /// 表示にごく僅かな差異(観測上小数第2〜3位、絶対値0.02未満)が生じるのは想定内のノイズであり、
    /// 本テストが検出すべき回帰ではない。そのためリテラル一致ではなく数値だけ許容誤差付きで
    /// 比較し、アイテム名・個数・構造・並び順など決定的な部分は厳密一致を要求する。
    const NUMERIC_TOLERANCE: f64 = 0.02;

    fn number_pattern() -> &'static Regex {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| Regex::new(r"-?\d+\.\d+").expect("number pattern must compile"))
    }

    /// 数値部分とそれ以外に分割する。偶数indexが非数値テキスト、奇数indexが数値文字列。
    fn tokenize(s: &str) -> Vec<&str> {
        let re = number_pattern();
        let mut tokens = Vec::new();
        let mut last_end = 0;
        for m in re.find_iter(s).flatten() {
            tokens.push(&s[last_end..m.start()]);
            tokens.push(&s[m.start()..m.end()]);
            last_end = m.end();
        }
        tokens.push(&s[last_end..]);
        tokens
    }

    /// 数値だけ許容誤差付きで比較する行単位の一致判定。
    fn line_matches(got: &str, expected: &str) -> bool {
        let got_tokens = tokenize(got);
        let exp_tokens = tokenize(expected);
        if got_tokens.len() != exp_tokens.len() {
            return false;
        }
        got_tokens.iter().zip(exp_tokens.iter()).enumerate().all(|(i, (g, e))| {
            if i % 2 == 0 {
                g == e
            } else {
                match (g.parse::<f64>(), e.parse::<f64>()) {
                    (Ok(gv), Ok(ev)) => (gv - ev).abs() <= NUMERIC_TOLERANCE,
                    _ => g == e,
                }
            }
        })
    }

    /// 行頭の順位番号("101. "や masterStats形式の"1.")を取り除く。隣接swap許容チェックで
    /// 順位番号そのもの(位置に紐づき、入れ替え検出には無関係)を比較対象から外すために使う。
    fn strip_rank_prefix(line: &str) -> &str {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        let re = PATTERN.get_or_init(|| Regex::new(r"^\d+\.\s*").expect("rank prefix pattern must compile"));
        match re.find(line) {
            Ok(Some(m)) => &line[m.end()..],
            _ => line,
        }
    }

    /// ランキング形式のチャンクで、僅かな理性価値の差(上記ノイズ)により隣接2件の順序が
    /// 入れ替わっただけのケースを許容する（例: 差が0.01未満の2件が前後する）。それ以外の
    /// 不一致（3件以上のずれ、値が離れすぎている等）は素直に不一致として報告する。
    fn lines_match_with_adjacent_swap_tolerance(got: &str, expected: &str) -> bool {
        let got_lines: Vec<&str> = got.split('\n').collect();
        let exp_lines: Vec<&str> = expected.split('\n').collect();
        if got_lines.len() != exp_lines.len() {
            return false;
        }
        let mut i = 0;
        while i < got_lines.len() {
            if line_matches(got_lines[i], exp_lines[i]) {
                i += 1;
                continue;
            }
            // 順位番号は位置(i, i+1)に紐づくため、入れ替え判定では中身(名前・値)だけを見る。
            let swapped = i + 1 < got_lines.len()
                && line_matches(strip_rank_prefix(got_lines[i]), strip_rank_prefix(exp_lines[i + 1]))
                && line_matches(strip_rank_prefix(got_lines[i + 1]), strip_rank_prefix(exp_lines[i]));
            if !swapped {
                return false;
            }
            i += 2;
        }
        true
    }

    /// 不一致をパニックせず文字列で返す（複数ケースの不一致を1回のテスト失敗で
    /// まとめて報告できるようにするため）。
    fn diff_against_golden(name: &str, reply: &EmbedReply) -> Option<String> {
        let golden = load_golden(name);
        if reply.title != golden.title {
            return Some(format!("[{name}] title mismatch:\n  got:      {:?}\n  expected: {:?}", reply.title, golden.title));
        }
        if reply.chunks.len() != golden.contents.len() {
            return Some(format!(
                "[{name}] chunk count mismatch: got {} chunks, expected {}\n  got:      {:#?}\n  expected: {:#?}",
                reply.chunks.len(),
                golden.contents.len(),
                reply.chunks,
                golden.contents
            ));
        }
        for (i, (got, expected)) in reply.chunks.iter().zip(golden.contents.iter()).enumerate() {
            if line_matches(got, expected) {
                continue;
            }
            if lines_match_with_adjacent_swap_tolerance(got, expected) {
                continue;
            }
            return Some(format!(
                "[{name}] chunk[{i}] mismatch (tolerance={NUMERIC_TOLERANCE}):\n  got:      {got:?}\n  expected: {expected:?}"
            ));
        }
        None
    }

    async fn build_state() -> AppState {
        // ModerationState::from_env()がAUTODEL_1等の環境変数を要求するため、main.rsと同様に.envを読む。
        dotenvy::dotenv().ok();
        let recruit = engine::recruit::RecruitEngine::load().expect("recruit data should load");
        let moderation = ModerationState::from_env();
        let outer_source = engine::outer_source::OuterSourceRegistry::load().await;
        let risei_calculator = engine::risei_calculator_engine::RiseiCalculatorEngine::load(&outer_source)
            .await
            .expect("risei engine should build against real network data");
        let fk_data_search = engine::fk_data_search::FkDataSearchEngine::new();
        AppState {
            recruit,
            moderation,
            outer_source,
            risei_calculator,
            fk_data_search,
        }
    }

    #[tokio::test]
    #[ignore]
    async fn operator_cost_calc_matches_python_golden() {
        let state = build_state().await;
        let mut failures = Vec::new();

        let reply = elite_cost_reply(&state, "ケルシー・エスペランタ").await;
        if let Some(diff) = diff_against_golden("elite_kelsey_esperanza", &reply) {
            failures.push(diff);
        }

        let reply = master_cost_reply(&state, "アーミヤ(前衛)", 1).await;
        if let Some(diff) = diff_against_golden("master_amiya_warrior_s1", &reply) {
            failures.push(diff);
        }

        let reply = module_cost_reply(&state, "アステシア").await;
        if let Some(diff) = diff_against_golden("module_astesia", &reply) {
            failures.push(diff);
        }

        let reply = cost_list_reply(&state, &CostListSelection::Star6Elite, false).await;
        if let Some(diff) = diff_against_golden("list_star6_elite", &reply) {
            failures.push(diff);
        }

        let reply = cost_list_reply(&state, &CostListSelection::MasterStar5, false).await;
        if let Some(diff) = diff_against_golden("list_master_star5", &reply) {
            failures.push(diff);
        }

        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    }
}
