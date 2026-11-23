# api/fk_kill_calculator（フレームキル計算機の画面）

`/FrameKillCalculator`。日本語専用（/en 無し）。カタログを組み立てるデータ層は
`src/engine/fk_kill_calc/CLAUDE.md` を参照。

## どこから読むか

- 計算層は `static/engine.js`（入口。公開する名前と単位の約束を冒頭コメントに持つ）と `static/engine/`。
- 表現層は `static/ui.js`（入口）と `static/ui/`。状態・保存・共有 URL は `static/ui/store.js`。
- ルーターは `mod.rs`。計算層は `verify.mjs`、画面は `e2e.mjs` で確かめる（実行方法はルートの CLAUDE.md）。

## 層の約束

- **計算層は `document` / `window` を参照しない**（verify.mjs が node で読み込むため）。
- **表現層は計算層を入口の `../engine.js` からだけ import する**。
- 画面は 420px 想定の縦長1カラム・ダーク固定。配色トークンは `home_index.html` の `:root` にある。
- `static/lz-string.min.js` は外から持ってきたもの（1.5.0, MIT）。手で直さない。

## static の版つき URL と no-cache

テンプレートは `/FrameKillCalculator/static/v2/…`（`mod.rs` の `STATIC_VERSION`）を読み、従来の `/static` も残す。
モジュールは相対 import なので入口（`ui.js`）の URL が変われば配下も全部新しい URL になり、古いキャッシュの JS と
新しい JS が混ざらない。**モジュール間で意味が変わる変更（例: 倍率の入力欄を % にした）をしたら `STATIC_VERSION` と
`fkc_index.html` の参照を上げる**。加えて static 応答は `no-cache`（毎回サーバーに確かめ、変わっていなければ 304）。

## catalog.json だけ web_ui_router() に入れない

ページ本体と static は `web_ui_router()` に足すが、`catalog_router()` は別軸。
`run_api`（本番。ExternalSourceRegistry の現在値からリクエスト毎に `build_catalog`）と
`serve_web`（dev。起動時に `data/seed/*.json` から1回だけ組み立てたものを使い回す）で取得方法が
全く違い、状態を持たない `web_ui_router()` に AppState 依存を混ぜられないため。
`CatalogProvider`（クロージャ）を両者がそれぞれ組み立てて `catalog_router(provider)` に渡し、個別に `merge` する。

## 計算（engine.js）

```
final = ((atk + buffFlat) × (1 + pct) + inspireFlat) × multiplier × ...
```

- **判定ロジックを Rust と JS に重複させない**: 条件付き/個別バフや乗算系特殊強化の値は、
  Rust が作った値テーブルを `resolveConditionalValue` / `resolveTalentTableValue` で引くだけ。
- **ATK**: `computeBaseAtk` が `phases[i]`（Lv1 / Lv最大の2点）を線形補間して **`Math.round`**
  （切り捨てだと実測と食い違う。実機確認済み: エーベンホルツ E2 Lv60=1310、シー E2 Lv71・信頼度100%・無凸で 1031）。
  信頼度は `atk_trust_max × trust/100` を別に四捨五入して足す。
- **潜在**: `row.potential`（0〜5。既定5＝潜在6）で `resolveAtkPotential` が
  `atkPotentialByRank[rank]` を引く。選択肢は `atkPotentialGroups(op, entry, row)` で、値が変わる
  境目だけにまとめる（境目が無ければセレクトを隠す）。乗算系特殊強化があると素質テーブルの境目も
  合成する（例: ファイヤーウォッチは「潜在1-3 / 4 / 5-6」）。鼓舞ソースは `inspirePotentialGroups`
  （ATK 加算の境目 ∪ 素質凸の解放境目。濁心スカジも3択）。
- **モジュール**: `effectiveModuleId(op, rowOrCfg)` が「選択中のモジュールを今の elite / level で
  装備できるか（`moduleUsable`）」を1か所で判定する。ATK・特殊強化・鼓舞の自己%パーツのモジュール条件は
  全部これ経由。装備できない間は効果に含めず、UI は「昇進X Lv Y以上で装備可能」を出す。
- **スキルLv**: `row.skillLevel`（既定10＝特化3）を `resolveEntryValues` / `resolveInspireRatioAtLevel`
  で解決。スキルLv や entry を変えると倍率/セルフ%欄をその時点のカタログ既定値へ再スナップする。
- **警告は出すが計算は続ける**（オーナー方針）: `skillUnlockWarning`（スキル未解放）、
  `maxSkillLevelForElite` / `skillLevelWarning`（E0 は SLv4、E1 は SLv7 まで、特化は昇進2）。
  昇進を変えてもスキルLv は自動で変えない。
- **特殊強化**: 加算系は `resolveSpecialAddPct`、乗算系は `resolveSpecialMultiplierFactor`
  （値0＝素質未解放なら ×1）。条件を満たさない間はチェックボックスの代わりにヒントを出す
  （`specialUiState` / `specialMulCanApply`）。ⓘの「現在: +N% / ×N」は
  `resolveSpecialCurrentValue` が出す「今 ON にしたら」の値（乗算系の0は1として見せる）。
- **バフ**: `computeBuffBreakdown`。条件付きは `exclusive_group` 内で両方 ON（共有 URL 等）なら最大1件だけ。
  `findSingleTargetConflicts` は行と鼓舞ソースの選択を同じ名前空間で数える（OFF のソースは数えない）。
- **FK 行との育成設定リンク**: `source` を持つバフは、同じオペレーターの FK 行があれば
  `state.globalBuffLevels` ではなくその行の昇進/潜在/モジュール/スキルLv を使う
  （`effectiveBuffLevels` / `findLinkedRow` / `computeEffectiveGlobalBuffLevels`。トグルはユーザー操作のまま）。
  状態には持たせず `state.rows` から都度計算するので、行を消せばリンクも外れる。
- **鼓舞**: ON のソースのうち**振れ幅が最大の1件だけ**を `inspireFlat` に足す（合算しない）。
  ソース自身の行には乗らない。`鼓舞 = ソースATK × (1 + 自己%パーツ + 個別% + 条件付き% + 手入力%) × スキル比率`
  （`computeInspireSource`）。バフの判定は行と同じ `computeBuffBreakdown` をソースの `tags` 込みで使う。
- **`row.buffFlat`**（手入力の基礎攻撃力+）: 行自身の%バフ前に足す。鼓舞ソースの ATK には乗らない。
  式表示は `(691+150)` のように出す。
- **混合スキル**: 行のトップレベル（multiplier / dmgType / dmgMult）が常にパーツ0、2番目以降は
  `row.extraParts`（トップレベルへのミラーは作らない）。Hit 数は全パーツ共通（オーナー指示）。
  `expandRowParts(row)` が疑似行の配列を返し、`computeTotal` はパーツごとに `computeRowDamage` して
  合算する（バフ・鼓舞・特殊強化は行で1回だけ計算）。結果は従来フィールド＋`parts`。
  `suggest` の Hit 数提案は行単位で1件。

- **碁石盤（ウァン S3）**: `entry.stoneBoard` があるエントリは Hit 数欄の代わりに5x5の盤面を出す
  （`row.stones` はマス番号の配列、中央12は赤マスで置けない）。ルールは `engine/stone_board.js` の冒頭コメント
  （スタックは縦横の線のうち多い方の個数で、自分も数える。オーナーの実測で確認済み）。`computeTotal` は同じスタック数の碁石を
  1パーツにまとめ、倍率に `1 + n×増加率`、敵の `resFlat` に `n×貫通` を足した疑似行で計算する（`parts` に
  `stacks` / `count`）。全碁石が同時に1回ずつ発動する扱い（実機ではスタックが下がらないので、命中タイミングの
  ずれはダメージに影響しない）。実機のスタックは石ごとの履歴（入った一番長い線）だが、盤面は履歴を持たず今の線を数える。
  5x5 の外に伸びる線も数えない。`verify.mjs` にオーナーの実測値（ATK 729 / 669）を入れてある。

## 状態と互換（ui.js / dropStaleRows）

- 状態は localStorage に自動保存し、**アドレスバーの URL は書き換えない**（ページ自体を共有しやすく
  するため）。`#state=` 付き URL は「共有URLをコピー」時だけ作り、開いたら読み込んで消す。
- state の `v` は 1 のまま。後から足したフィールドは省略可能で、`dropStaleRows` が欠損補完・
  未知のバフ id / 鼓舞ソース id / パーツ id の除去を兼ねる。
- **まだプロトタイプなので丁寧な移行はしない**（オーナー指示）。合わない値は既定値に戻す
  （例: `potential` が数値でなければ5、`extraParts` の形が合わなければ作り直す）。
  例外として残っている移行: 前衛アーミヤ旧2エントリ → `amiya_guard`＋toggle、
  ステインレス旧 `stainless_1` / `stainless_2` → `stainless_s1`＋toggle。

## 再描画

- 画面の状態は `ui/store.js` の `export let`（`state`・`catalog`・`expandedIdx` など）。読むのは import した名前の
  ままでよい（値の変化は見える）が、import した名前には代入できないので、書き換えは `setState` などの setter を通す。
- 構造が変わる操作は `render`（`#app` 丸ごと。フォーカスは `withPreservedFocus` で戻す）。
- 数値欄の打鍵中は `renderLive`（入力欄を作り直さず導出表示だけ差し替え。type=number は
  カーソル位置を戻せないため）。チップ・チェックボックス・select は常に `render`。
- `ROW_PERCENT_FIELDS` は%換算する欄（`selfPct` / `buffPct` / `dmgMult` / `multiplier`）。`buffFlat` は入れない。
  `multiplier` は状態が小数（0.5）のまま、入力欄の表示・読み取りだけ % （50）。倍率候補セレクトの value も小数で、
  ラベルだけ %。バッジ判定（`fieldBadges`）に渡す現在値は小数。
- 混合スキルの入力要素は `data-part="<n>"`（0は省略）を持ち、`onRowFieldChange` / `renderLive` /
  reset-field は `partTarget` で row 自身か `row.extraParts[n-1]` かを決める。
  `renderDamagePartsSection` が「スキルの値」の中身（パーツごとのブロック。パーツ有りでは倍率候補セレクトを出さない）を描く。
  式表示（`renderFormulaLine`）はパーツごとに1行＋合計行。
- 行のオペレーターが確定した瞬間（`onOperatorNameChange`）だけ、そのオペレーター由来の条件付きバフを
  `autoEnableSourcedBuffs` で ON にする。ユーザーが OFF にしたら他の操作で再 ON しない。
- 「個別バフの育成設定」（`renderIndividualBuffLevelsSection`）は、今チェックされている source 付き
  個別バフだけをカード表示する。軸の選択 UI は条件付きと共通の `renderBuffAxisControls`
  （リンク中は「FK行（<name>）の設定を使用」）。

## レイアウト

- 判定欄（`#verdict-section`）は上部 sticky。ツール切り替えバーも上端 sticky なので、その高さ
  （ui.js が実測して `--toolnav-h` に入れる）だけ下に貼る。
- 判定は `#app` ごと差し替わって読み上げに載らないので、`renderVerdict` が作る平文を `#verdict-live`
  （`#app` の外・`visually-hidden`・`role=status`）へ 600ms デバウンスで反映する（前回と同文なら何もしない）。
- `updateVerdictGlow` が body に `verdict-glow-ok` / `-ng` を付け、`body::after` で縁を光らせる（HP 未入力は無し）。
- 行の展開ビューの組の順番（`renderRowExpanded`）: 折りたたむ → 行の頭（名前＋「この行のダメージ」`row-total`）→
  スキル（FK対象・スキルLv・警告・タグ・Hit数/碁石盤）→ 育成 → バフ → 「スキルの値」（`details.auto`）→ 式 → FK情報 →
  行フッター（`× この行を削除` / 折りたたむ）。オペレーター未選択の間は名前欄と一言とフッターだけ。
- 「スキルの値」は倍率・ダメージ種別・セルフ・防御無視など自動で埋まる欄。開閉は `store.js` の `autoOpenIdx`
  （`ui.js` の toggle リスナーが更新、描画時に `open`。`specialDescOpenIdx` と同じく行の削除・複製による idx のずれは追わない）。
  e2e で中の欄を触るときは先に開く（`openAuto`）。
- `renderLive` は開いている行の式・バッジ・モジュールヒントに加え、行の合計（`[data-role="row-total"]`）と
  summary の「今の値」（`row-auto-now`）・「変更あり」（`renderAutoSummaryDynamic`）も差し替える。
- 展開ビューの先頭にも `collapse-row` ボタン。撃破提案は折りたたみ、共有ボタン・注記はページ末尾。
- 折りたたみ行のサマリーは `E2 Lv90`（信頼度100%は省略）、特化3以外ならスキルLv（`SLv7`）を足す。
  混合スキルはパーツ内訳を tooltip に入れる。
