# api/fk_kill_calculator（フレームキル計算機の画面）

`/FrameKillCalculator`。日本語専用（/en 無し）。カタログを組み立てるデータ層は
`src/engine/fk_kill_calc/CLAUDE.md` を参照。

## ファイル

| ファイル | 役割 |
|---|---|
| `mod.rs` | `page_router()`（ページ本体＋`/static`）と `catalog_router(provider)`（`GET .../catalog.json`） |
| `templates/fkc_index.html` | toolnav / head_icons / og_square_image を include。SEO タグは `base_url()` から |
| `static/engine.js` | 計算層。`document` / `window` を参照しない。単位の約束はファイル冒頭コメント |
| `static/ui.js` | 表現層。カタログ fetch・状態管理・DOM 描画 |
| `static/style.css` | 420px 想定の縦長1カラム、ダーク固定（配色トークンは `home_index.html` の `:root`） |
| `static/lz-string.min.js` | 1.5.0, MIT |
| `verify.mjs` / `e2e.mjs` | 計算層の検証 / Playwright での実機テスト。実行方法はルートの CLAUDE.md |

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

- 構造が変わる操作は `render`（`#app` 丸ごと。フォーカスは `withPreservedFocus` で戻す）。
- 数値欄の打鍵中は `renderLive`（入力欄を作り直さず導出表示だけ差し替え。type=number は
  カーソル位置を戻せないため）。チップ・チェックボックス・select は常に `render`。
- `ROW_PERCENT_FIELDS` は%換算する欄。`buffFlat` は入れない。
- 混合スキルの入力要素は `data-part="<n>"`（0は省略）を持ち、`onRowFieldChange` / `renderLive` /
  reset-field は `partTarget` で row 自身か `row.extraParts[n-1]` かを決める。
  `renderDamagePartsSection` がパーツごとのブロックを描く（パーツ有りでは倍率候補セレクトを出さない）。
  式表示（`renderFormulaLine`）はパーツごとに1行＋合計行。
- 行のオペレーターが確定した瞬間（`onOperatorNameChange`）だけ、そのオペレーター由来の条件付きバフを
  `autoEnableSourcedBuffs` で ON にする。ユーザーが OFF にしたら他の操作で再 ON しない。
- 「個別バフの育成設定」（`renderIndividualBuffLevelsSection`）は、今チェックされている source 付き
  個別バフだけをカード表示する。軸の選択 UI は条件付きと共通の `renderBuffAxisControls`
  （リンク中は「FK行（<name>）の設定を使用」）。

## レイアウト

- 判定欄（`#verdict-section`）は上部 sticky。ツール切り替えバーも上端 sticky なので、その高さ
  （ui.js が実測して `--toolnav-h` に入れる）だけ下に貼る。
- `updateVerdictGlow` が body に `verdict-glow-ok` / `-ng` を付け、`body::after` で縁を光らせる（HP 未入力は無し）。
- 展開ビューの先頭にも `collapse-row` ボタン。撃破提案は折りたたみ、共有ボタン・注記はページ末尾。
- 折りたたみ行のサマリーは `E2 Lv90`（信頼度100%は省略）、特化3以外ならスキルLv（`SLv7`）を足す。
  混合スキルはパーツ内訳を tooltip に入れる。
