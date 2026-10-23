# engine/recruit（公開求人のタグ計算）

OCR の生テキストからタグを抜き出し、組み合わせごとの確定★を計算して整形する。
bot にも api にも依存しない。入力のプールは `external_source::recruit_pool` の DTO
（作り方は `src/engine/external_source/CLAUDE.md`）。

| ファイル | 役割 |
|---|---|
| `mod.rs` | `RecruitEngine`。`data()` で Arc のスナップショット、日次更新で `rebuild(pool)` して丸ごと差し替え |
| `matcher.rs` | OCR 生テキスト → タグ（fancy-regex、3言語辞書＋誤字補正） |
| `calc.rs` | タグ計算（ピックアップ対応。プールの jp = main、cn_only = future） |
| `format.rs` | 出力整形（`display_chunks` / `response_for_ai` / `make_title` / 分割） |
| `jp_release.rs` | CN 追加分の JP 反映日（`data/recruitment/jp_release.yaml`。`include_str!`） |

## 守ること

- **matcher の補正辞書は資産**: Google Vision と Apple OCR の実データで調整してあり、99%以上通る。
  辞書や正規表現を変えるときは既存の挙動を崩さない。
- **fancy-regex を使う**: `(?!上級)` のような否定先読みが標準 regex では書けない。
- **分割は文字数で数える**: format.rs は `chars().count()`。`String::len()`（バイト数）にすると
  日本語で分割位置がずれる。
- **並び順**: `sort_items` は安定ソートの重ねがけ（keyLen desc → valueLen asc → maxstar desc →
  minstar desc → containsPickup desc。最後が最優先）。順序を変えると結果の並びが変わる。
- **`star_set` は BTreeSet**: ★表記（★4,5 など）を昇順で安定させるため。HashSet にしない。
- **future（CN 追加分）の扱い**: `jp_release.yaml` の日付の `RECRUIT_FUTURE_HOUR` 時（既定 16:00 JST）を
  過ぎたら、JP のゲームデータに無くても JP として扱う（データ更新の遅れ対策）。それまでと未設定の間は
  大陸版でだけ対象。JP のデータが追いつけば自動で jp 側に移るので、YAML に残った行は無視される。
  YAML の名前がプールに無い（タイプミス）・同じオペレーターを2回書いた場合は `cargo test` が落ちる。
