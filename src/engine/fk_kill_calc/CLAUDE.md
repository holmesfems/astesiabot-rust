# engine/fk_kill_calc（フレームキル計算機のデータ層）

フレームキル計算機（`/FrameKillCalculator`）のカタログを組み立てる、Rust 側のマージ層。
bot にも api にも依存しない。画面側（engine.js / ui.js）は
`src/api/fk_kill_calculator/CLAUDE.md` を参照。

## 3層構成

1. **機械データ**: `external_source::operator_combat`（ATK・潜在・モジュール・素質・
   昇進/レベル）＋ `skill_data`（blackboard / blackboard_by_level / is_ammo_skill）
2. **手動補正**: `data/fk_kill_calc/overrides.yaml`（`overrides.rs`）と
   `data/fk_kill_calc/buffers.yaml`（`buffers.rs`）。どちらも `include_str!` で埋め込み、
   `global()` でプロセス内1回だけパースする。スキーマ・記入例は各 YAML の冒頭コメント
3. **マージ**: `mod.rs` の `build_catalog`（fk_data 起点）

手動補正が要るのは、blackboard のキー名が `atk_scale` でない・1スキルが物理/術の2系統を
持つなど、機械的に正しく判定できない実データがあるため。DTO の `Valued<T>{value,source}` が
各値の出所（`Auto` / `Manual`）を持ち、画面の「補正」バッジに使う。

## ファイル

| ファイル | 役割 |
|---|---|
| `mod.rs` | `build_catalog`、`resolve_multiplier_defaults`、`build_inspire_sources`、`build_combined_entry`、各種 `validate_*`（ドリフト検知） |
| `conditional_source.rs` | バフの値をゲームデータから解決して値テーブルを作る（`build_conditional_sourced_buffers` / `build_individual_sourced_buffers` → 共通の `build_source`）。アルゴリズムはファイル冒頭コメント |
| `dto.rs` | `Catalog` / `CatalogOperator` / `FkEntry` / `FkPart` / `Buffer` / `InspireSource` / `Conditional*Source` 等。JSON は camelCase |
| `tags.rs` | profession/position/nationId からタグ・ダメージ種別の初期値を推測。`tag_vocabulary()`（全タグ語彙。YAML のタグのドリフト検知用）、`faction_tag_for`（今はラテラーノのみ） |
| `overrides.rs` / `buffers.rs` | 上記 YAML のロード。`buffers.rs` の `raw_conditional_sourced()` / `raw_individual_sourced()` / `raw_inspire_sources()` は生データだけ返し、ゲームデータとのマージは `mod.rs` / `conditional_source.rs` 側（2段構え） |

## 守ること

- **名前解決**: fk_data シート → ゲームデータは、全角/半角括弧と前後空白を
  `normalize_operator_name` で正規化してから引く。解決できない名前は `skipped` に集める。
  バフ・鼓舞ソースも、参照先（operator / talentIndex / skill_num / module 等）が実データに
  無ければ `skipped` に `"buff:<id>"` / `"inspire:<id>"` として記録し、静かに落とす。
- **倍率の自動判定**: 厳密一致の `atk_scale` → 無ければキーが `atk_scale` / `damage_scale` で
  終わる項目のうちアルファベット順で最初 → どちらも無ければ 1.0。候補一覧は
  「デフォルトが先頭、残りはキー名のアルファベット順」に正規化する
  （`write_seed_file` がキーをソートするので、Seed 経由と実 fetch で並びが揺れないように）。
- **スキルLv別の値**: `FkEntry` は `multiplierByLevel` / `selfAtkPctByLevel`（Lv1〜10。
  8〜10＝特化1〜3）を持つ。`multiplier` / `self_atk_pct` は後方互換で最終レベルの値。
  overrides の `multiplier_key` と固定 `multiplier`、`self_atk_pct_factor`（Auto 値に係数。
  `round4` で丸める）と固定 `self_atk_pct` はそれぞれ排他（`validate_override_level_fields`）。
  鼓舞ソースも固定 `ratio` の代わりに `ratio_key` でスキルLvに追従できる。
- **タグ**: `FkEntry.tags` ＝ オペレーター機械タグ（近距離/職業/勢力）＋スキル機械タグ
  （`弾薬スキル` ＝ `durationType == "AMMO"`）＋ overrides の手動 `tags`（加算）。
  `全員`（`tags.rs::ALL_TAG`）はどの行にも一致する特別なタグで、エントリ側には載せない。

## buffers.yaml の仕組み

- **individual**（個別）: 行ごとにチップで選ぶ。タグ判定なし。
- **conditional**（条件付き）: 全体で1回 ON/OFF し、`FkEntry.tags` と `targets` が1つでも
  重なれば自動適用。`exclusive_group` が同じものは同時に効かない。
- 値は固定 `pct` / `flat`、またはゲームデータからの `source` のどれか1つ。
- `source` の種類（`conditional_source.rs` が scope を問わず共通で解決する）:
  - `talent` + `key`: 素質。`operator_combat.talents[i].candidates`（phase × potentialRank）＋
    モジュールの `talent_overrides_by_level` から 昇進×潜在（×モジュールLv）の表を作る
  - `skill_num` + `key`: スキルLv1〜10 の blackboard
  - `scale_skill_num` / `scale_key`: 素質値または `base_pct` に掛けるスキルLv別の2軸目
    （例: スワイヤー S1/S2）
  - `base_pct`: ゲームデータに無い固定基礎値（例: ステインレス S1 の装置本体 +12%）
  - `stage_skill_id` + `stage_keys` + `stage_labels`: 同じスキルの複数キーを段階として使う
    （例: ナスティ S3 のアップグレード1〜3段階）
- **値が変わらない軸は自動で隠す**（`elite_varies` / `potential_varies`、値を変える
  モジュールだけ `modules` に残す、`scale.varies`）。例: ポデンコ/ペペはモジュール上書きが
  ベースと同値なので `modules` が空。スズランはベースにキーが無く、モジュールXで初めて値が付く。
- `Buffer.value` には最大成長（昇進2・潜在6・値を変えるモジュールがあればLv3・スキルは最大Lv）
  で解決した値を入れる（`ConditionalSourceDefaults`）。
- `toggle: { label, mult }`: ON/OFF で解決値に倍率を掛ける（前衛アーミヤ「スキル中は2倍」、
  ステインレス「装置2台」）。
- `bonus`（任意）: `bonus.targetTags` にも重なれば基本値を**置き換える**。`value`（固定）か
  `mult`（基本値への倍率。例: ラテラーノ勢は ×2 = 26%。実データの blackboard にも
  `mult` = 2.0 がある）。
- `max_targets_by_module`（individual のみ）: 指定モジュールLv以上の間、`single_target` 警告の
  許容数を緩める（例: エクシアはモジュールX Lv2 以上で2名まで）。値の解決には関与しない。
- 血漿（plasma）・ドリアン（durian）は「自身＋ランダムな味方1名」なので `single_target`。
- ホルン「軍事要塞」（`horn`）は素質由来の conditional（`targets: [重装]`）。旧 overrides の
  S2 固定 `self_atk_pct: 0.31` をここへ移した。ホルンの初期モジュールは overrides の
  `default_module: uniequip_002_horn`（X）。汎用の既定（Lv3 の ATK 加算が最大＝Y）だと
  この素質が強化されず実効 ATK が下がるため。`default_module` はオペレーター単位なので、
  選んだエントリに無ければ同じオペレーターの他エントリの指定を使う。

## 鼓舞（`buffers.yaml` の `inspire`）

データ側の約束だけここに書く（計算式と「最大値1件だけ適用」は画面側の CLAUDE.md）。

- `self_parts`（自己%条件）は特定オペレーターにハードコードしない汎用スキーマ:
  `pct` + `pct_potential_bonus`（素質凸で加算）が基本形。
  - `module_override`: 指定モジュールのLv1〜3で `pct` 自体を置き換える（素質凸ボーナスは
    置き換え後に乗る）。例: 濁心スカジの `talent` / `talent_abyssal` はモジュールYで強化。
  - `requires_module` + `pct_by_module_level`: そのモジュール装備時だけ存在する別効果
    （素質凸とは無関係）。例: モジュールXの「2名以上」は実データで Lv1〜3 とも 0.08。
  - この2系統は排他（同時に装備できるモジュールは1つ）。
  - `replaces`: 他のパーツを加算でなく置き換える（例: 「範囲内にアビサルハンター」は「素質」を置き換え）。
- `talent_potential_rank`（0始まり）: `pct_potential_bonus` が効き始める潜在。実データの
  `requiredPotentialRank` を写す。

## 特殊強化（overrides.yaml の `special`）

`label` 必須、他は省略可。2系統あり併用できる。値は行に写さず、毎回計算し直す
（モジュールを変えれば即座に反映されるように）。

- **加算系**: `requires_module` + `add_self_atk_pct_by_module_level`（要素数3、効果の無いLvは0）。
  指定モジュール装備時だけセルフ%に加算。例: ブレイズ S3「待機ボーナス」（X: 0/0.04/0.06）、
  ウィーディ S3「蓄水砲配置バフ」（X: 0/0.15/0.20）。
- **乗算系**: `mul_multiplier: { talent: <index>, key: <blackboardキー> }`。条件付きバフの
  `source.talent` と同じビルダー（`conditional_source::build_talent_source`）で値テーブルを作り、
  行の昇進/潜在/実効モジュールで引いて倍率に掛ける。例: ファイヤーウォッチ S2「遠距離特効」
  （素質「暗殺者」。E1=1.2/1.25、E2=1.4/1.45［潜在1〜4/5〜6］、モジュールY Lv2=1.45/1.5、Lv3=1.5/1.55）。
- `description`（短い説明文）は画面のⓘで表示する。
- **モジュール依存の値は必ず実データで確かめる**: `battle_equip_table.json` の該当 uniEquipId の
  `addOrOverrideTalentDataBundle` を Lv ごとに読む。Lv1 は素質強化自体が付かず0のことが多い。
  1つの定数を全 Lv に当てはめない（初期実装の「置き換え系」はこれで実態と合わず廃止した）。

## 混合スキル（物理＋術を同時に出す。例: ホルン S2）

- overrides の `combined: true`（省略時 false）が付いた、同じ skill_num のバリアント群を
  `build_combined_entry` が1つの `FkEntry` にまとめる。物理のみ/術のみの単独選択肢は無くした
  （オーナー承認済み）。
- 先頭バリアントがパーツ0（`FkEntry` のトップレベルの値）。hits / self_atk_pct / special /
  tags / note / default_module は先頭のものだけを使う。2番目以降がこれらを持たないこと、
  `default_module` を持つなら先頭と同値であることを `validate_combined_variants` が検証する。
- `variant_label` はパーツラベルを `+` で連結（例: `物理+術`）。`FkEntry.parts` は先頭も含む
  全パーツ（`{label, multiplier, multiplierByLevel, multiplierFixed, damageType}`）。通常エントリは空で JSON にも出さない。

## ドリフト検知（`cargo test`）

ゲームデータ更新で落ちたら YAML を見直す。

- `every_override_key_points_to_existing_operator_and_skill_num`
- `validate_override_level_fields` / `validate_combined_variants`
- `validate_special_requires_module`（モジュールが実在）/ `validate_special_mul_multiplier`
  （talent/key が実在し、最大成長で0より大きい）
- `validate_conditional_sources` / `validate_individual_sources`（参照の実在、最大成長で0以下にならない）
- `every_inspire_source_points_to_existing_operator_skill_and_modules`、
  `every_inspire_source_with_potential_bonus_has_talent_potential_rank`、
  `buffers.rs` の `inspire_self_part_replaces_points_to_an_existing_part_in_the_same_source`
- `buffers.rs` の `#[cfg(test)]`: id の一意性、`pct` / `flat` / `source` のちょうど1つ、
  target タグが `tag_vocabulary()` にある、値が現実的な範囲
