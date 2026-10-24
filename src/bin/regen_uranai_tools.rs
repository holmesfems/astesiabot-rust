//! uranai(占い館)の`data/uranai/toolList.yaml`を`functioncalling::all_tools()`から
//! 再生成するツール。main.rs(bot/api本体)とは独立に動く。プロジェクトルートで
//! `cargo run --bin regen_uranai_tools` を実行し、差分を確認してcommit/pushする。
//! 関数を追加・変更したら（description/parameters変更含め）このツールを実行すること。

use astesiabot_rust::bot::services::uranai::functioncalling::build_tool_list_entries;

const HEADER: &str = "\
# 占い館(uranai)のOpenAI function calling用ツール定義。
# `cargo run --bin regen_uranai_tools` で自動生成される。手動編集しないこと。
# 生成元: src/bot/services/uranai/functioncalling/ に登録された各関数。
";

fn main() {
    let entries = build_tool_list_entries();
    // 長い description を `>-` で折り返さず、1行のまま出す（差分を読みやすく保つ）
    let options = serde_saphyr::ser_options! { prefer_block_scalars: false };
    let yaml = serde_saphyr::to_string_with_options(&entries, options).expect("tool list should serialize to YAML");
    let path = "data/uranai/toolList.yaml";
    std::fs::write(path, format!("{HEADER}{yaml}")).unwrap_or_else(|e| panic!("failed to write {path}: {e}"));
    println!("{path} を再生成しました。`git diff` で差分を確認してcommit/pushしてください。");
}
