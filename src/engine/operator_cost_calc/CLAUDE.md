# engine/operator_cost_calc（オペレーターの育成コスト）

昇進・スキル特化・モジュールの消費素材と、そのランキング/統計。bot にも api にも依存しない。
計算関数（`calc.rs`）は DTO（`dto.rs`）を返すだけで、文言化は bot / api 側
（順位は `RankingPosition{star,rank,total}`、文言は bot の `mastery_ranking_text` / `elite_ranking_text`）。

## 守ること

- **`ItemCost` は挿入順を保つ**（`model.rs`。中身は `IndexMap`、Cargo.toml で `serde_json` の
  `preserve_order` を有効化）。`normalize()` は value_target に無い項目（SoC 芯片等）をその時点の
  挿入順で安定ソートするので、`HashMap` / `BTreeMap` にすると表示順が変わる
  （例: モジュール消費でデータ補完チップ/マシンが複数種類同時に出るケース）。
- **直近実装の判定**（`AllOperatorsInfo::is_recent`）: CN 限定、または JP に初めて現れてから
  `RECENT_DAYS`（180日）以内。期限は参照のたびに JST の今日で決まるので、手で外す作業は無い。
  初登場日の記録は `src/engine/external_source/CLAUDE.md` の「JP 初登場日」。
- スキルの説明文（`SkillData::get_description`）は用意済みだが、特化コストの embed にはまだ出していない。
  出すなら `bot/commands/operator_cost_calc/operatormastercost.rs` から呼ぶ。
