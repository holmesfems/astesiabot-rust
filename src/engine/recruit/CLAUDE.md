# engine/recruit（公開求人のタグ計算）

OCR の生テキストからタグを抜き出し、組み合わせごとの確定★を計算して整形する。
bot にも api にも依存しない。入力のプールは `external_source::recruit_pool` の DTO
（作り方は `src/engine/external_source/CLAUDE.md`）。

流れは OCR の生テキスト → `matcher.rs`（タグ）→ `calc.rs`（計算）→ `format.rs`（整形）。
画像から直接タグを読むローカル OCR は `tag_ocr/`（下記）。

## 守ること

- **データは丸ごと差し替える**: `RecruitEngine` は Arc のスナップショットを返し、日次更新では作り直したものと入れ替える。

- **matcher の補正辞書は資産**: Google Vision と Apple OCR の実データで調整してあり、99%以上通る。
  辞書や正規表現を変えるときは既存の挙動を崩さない。
- **fancy-regex を使う**: `(?!上級)` のような否定先読みが標準 regex では書けない。
- **分割は文字数で数える**: format.rs は `chars().count()`。`String::len()`（バイト数）にすると
  日本語で分割位置がずれる（テスト: `chunk_text_counts_chars_not_bytes`）。
- **並び順**: `sort_items` は安定ソートの重ねがけ（keyLen desc → valueLen asc → maxstar desc →
  minstar desc → containsPickup desc。最後が最優先）。順序を変えると結果の並びが変わる（テスト: `sort_*`）。
- **`star_set` は BTreeSet**: ★表記（★4,5 など）を昇順で安定させるため。HashSet にしない（テスト: `calculate_orders_operators_and_star_set_by_stars_ascending`）。
- **future（CN 追加分）の扱い**: `jp_release.yaml` の日付の `RECRUIT_FUTURE_HOUR` 時（既定 16:00 JST）を
  過ぎたら、JP のゲームデータに無くても JP として扱う（データ更新の遅れ対策）。それまでと未設定の間は
  大陸版でだけ対象。JP のデータが追いつけば自動で jp 側に移るので、YAML に残った行は無視される。
  YAML の名前がプールに無い（タイプミス）・同じオペレーターを2回書いた場合は `cargo test` が落ちる。

## ローカルのタグ枠 OCR（tag_ocr/）

日本語表示の画面と軸が揃ったスクショなら、タグ枠 5 個（3+2 のグリッド。枠は暗灰・青＝選択中・黄＝エリート系）を
検出し、枠ごとにテンプレート照合してタグを読む。5 枠とも「1 位 ≥ T かつ 1 位と 2 位の差 ≥ M」で、全部違うタグなら
`TagOcrResult::Read`、それ以外は `Unsure { reason, boxes }`（グリッド無し・低スコア・重複）で Google Vision に回す。
英語・大陸版・モニターを撮った写真は枠が見つからないか自信が出ないので、自然に Google に落ちる。

入口は `tag_ocr/mod.rs` の `TagOcr::read`。処理は検出（`detect.rs`）→ 特徴（`feature.rs`）→ 採点（`score.rs`）の順。

- テンプレートは `data/recruitment/tag_templates/`（コミットする。約 140KB）。代表（prototype）1 つにつき
  グレースケール PNG 1 枚（`<タグ>_<番号>.png`、画素値 = 特徴値を 0..255 に量子化）と `meta.json`
  （T・M、特徴の大きさ・前処理と再判定の定数、タグごとの PNG の一覧、紛らわしい組）。
  **meta.json の定数がコードの定数と食い違うと `load` が失敗する**（前処理や再判定の定数を変えたらテンプレートを作り直す）。
  紛らわしい組は全組の相関を起動時に計算すると重いので、書き出し時に済ませて meta.json に持つ。
- 作り直し（手元。スクショは `local/` に集める。コミットしない）:
  `collect_recruit_samples`（スクショと bot の返信のタグを集める）→ `cut_tag_boxes`（枠を検出して切り抜く）→
  `fit_tag_templates --export data/recruitment/tag_templates`（時系列で古い 80% で学習・T, M を検証側で選び、
  量子化した PNG を `TagOcr::load` し元のスクショから読み直した評価も出す。`--export` は英語を除く全データで学習し直して書き出す）。
  目安: テスト誤読率 ≤ 1%・全部読み切れる率 ≥ 95%（今の値は fit_tag_templates が出す）、英語の画像の読み切りは 0。
  英語の画像は `local/recruit_samples/english_images.txt`（目で確認した手書きの一覧）。
- 検出・特徴・採点のコードは lib にだけある（`cut_tag_boxes`・`fit_tag_templates` もこれを呼ぶ）。
  学習側（EM・代表の分割・T, M の選択・評価）は `fit_tag_templates` にだけある。
- `cargo test --release -- --ignored confusable_pairs_match_recomputation read_timing` で、meta.json の紛らわしい組が
  PNG から計算し直した結果と一致することと、手元のスクショでの 1 枚あたりの時間（約 80ms）を確かめられる。
