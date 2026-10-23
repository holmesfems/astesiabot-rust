# engine/external_source（外部データのレジストリ）

外部サイトから取った情報を起動時にまとめて fetch してメモリに持ち、機能側はそこから読む。
bot にも api にも依存しない。fetch 失敗の扱いと Seed の運用はルートの CLAUDE.md
（全体のルール）を参照。

## 仕組み

- `mod.rs`: `ExternalSourceRegistry`。`load` / `refresh_all`（日次の一括 fetch）/
  `refresh_by_name`（機能側からの個別更新）。`SEED_JOBS`（regen_seeds が回す一覧）もここ。
  **情報源を増やす手順はこのファイルのコメント**。
- `cache.rs`: `Source<T>`。Seed は読むだけ。`write_seed_file` は regen_seeds 専用。
- `http.rs`: 全情報源共通の fetch（7s タイムアウト・最大3回リトライ）。巨大 JSON の一部だけ
  使うなら、`Value` を経由しない `fetch_typed_with_retry`。fetch ごとに独自のリトライを書かない。

## 情報源

| ファイル | 中身 | Seed |
|---|---|---|
| `ark_stages.rs` / `ark_matrix.rs` | penguin-stats の `/stages` / `/result/matrix`（matrix は CN 固定）。集計は risei_calculator_engine 側 | `ark_stages.json` / `ark_matrix.json` |
| `formulas.rs` / `item_names.rs` / `zones.rs` | 合成レシピ / アイテムの id・中国語名・日本語名 / zoneId → 日本語名 | 同名 `.json` |
| `operator_data.rs` | オペレーター CN→JA 名＋昇進/特化/モジュールの消費素材（character_table / uniequip_table / char_patch_table を1回で） | `operator_data.json` |
| `operator_combat.rs` | フレームキル計算機用の戦闘データ（ATK・信頼度・潜在ランク別 ATK・モジュール・素質・昇進ごとの Lv/ATK・nationId・スキル解放昇進） | `operator_combat.json` |
| `skill_data/` | スキル名・説明文・blackboard（最大Lv / Lv別）・`is_ammo_skill` | `skill_data.json` |
| `fk_data.rs` | FK 情報スプレッドシート（Google Sheets API v4）の行データ。`FK_SHEETS_*` を使う | `fk_data.json` |
| `recruit_pool.rs` | 公開求人のプール（`jp` / `cn_only`） | `recruit_pool.json` |
| `operator_birthdays.rs` | オペレーターの誕生日 | `operator_birthdays.json` |
| `jp_first_seen.rs` | JP に初めて現れた日の記録（Source ではない。下記） | `jp_first_seen.json` |

`data/seed/operator_names.json` は統合前の古い Seed。regen_seeds では更新しないが、
名前解決が壊れていないかのゴールデン参照として意図的に残している。

## 守ること

- **operator_combat と operator_data は意図的に別ソース**（オーナー方針: 消費素材ドメインと
  戦闘ドメインを混ぜない）。CN/JP マージ・名前解決・昇格オペレーターの扱いは同じ方針を
  複製している（依存を作らないための意図的な重複）。
- **モジュールの振り分けは `tmplId.unwrap_or(charId)`**（operator_data / operator_combat の両方）。
  uniequip_table のエントリは `charId` が基礎オペレーター（前衛アーミヤでも `char_002_amiya`）のままで、
  どの派生形専用かは `tmplId` が持つ。`charId` だけで振ると派生形専用のモジュールが基礎オペレーターに付く。
- **`skill_data/raw.rs` の `spData.spType` は数値も来る**（CN の PASSIVE 中心に600件超。例: イネス S3）。
  `string_or_number` で両方受ける。`String` に戻すと該当スキルが丸ごと欠落し、名前が "Missing" になる。
- **`skill_data/description.rs` は実データに即して正しく表示する**（フォーマット指定子の小数点、
  `chain.max_target` を `max_target` と混同しない等。詳細はファイル冒頭コメント）。
- **fk_data は日次の `refresh_all` に入れない**。`engine/fk_data_search` が読み取りのたびに
  「前回から1時間経っていれば再 fetch」する。オートコンプリートはこの TTL を通さず直接読む。

## 公開求人のプール（recruit_pool.rs）

- CN/JP の `gacha_table.json` の `recruitDetail`（ゲーム内の説明文）から名前と★、
  character_table から職業・位置・タグを引く。CN/JP の突き合わせは charId（名前だと表記変更で別人扱いになる）。
- JP の character_table に無いオペレーターは CN のタグを `tagZhToJa.yaml` で訳し、名前は
  `customOperatorZhToJa.yaml` の仮訳を使う。character_table は型付き fetch で必要なフィールドだけ読む。
- `recruitDetail` は人が書いた文章で表記ゆれがある（CN は改行が文字の `\n`、JP は `/` 前後の
  スペース抜けや全角スペース）。パーサーはこれを吸収する。
- 一方、名前が引けない・知らないタグ（tagList.json に無い）・★の食い違いは **fetch 失敗にする**
  （黙って落とすと計算結果が気づかれずに間違うため。失敗なら直前の値/Seed を使い続ける）。
  ゲームに新しいタグが増えたら tagList.json と OCR 辞書に足す。
- 同じ★の中の並びはゲームの掲載順（JP 分 → CN 追加分）。

## 誕生日（operator_birthdays.rs）

- CN の `handbook_info_table.json` の「基础档案」【生日】（ロボットは【出厂日】）から読む
  （JP 未実装もお祝いするため CN。CN/JP 両方にいる分は日付が全員一致）。
- 先頭の `M月D日` だけ読み、注記は無視する（GALLUS² の `5月4日（…推算）` 等）。「未公开」等は対象外。
  入手不可でも日付があれば対象（予備隊員）。
- `char_meta_table.json` の spCharGroups で異格をまとめる: 元と同じ日付なら元の1件に、
  元に日付が無ければ異格側を残す（凯尔希·思衡托・烛煌）。
- 表示名は JP 名 → customOperatorZhToJa → CN 名。同じ日の並びは charId 順。
- 1件も読めなければ fetch 失敗（書式変更で全員消えるのを防ぐ）。敬称・文面は bot 側。

## JP 初登場日（jp_first_seen.rs）

`/operatorcostlist` の「直近実装」判定（`AllOperatorsInfo::is_recent`）に使う。

- regen_seeds（`jp_first_seen::update_seed`）だけが更新し、実行時は `include_str!` で埋め込んだ記録を
  読むだけ（GitHub にアクセスしない）。
- ArknightsAssets の `jp/.../char_meta_table.json`（約75KB。JP で入手可能なオペレーターが全員載る）の
  コミット一覧を GitHub API で `since=checked_at` 以降だけ取り（`GITHUB_TOKEN` があれば使う）、
  最新版に載っていて記録に無いキャラだけを新しいコミットの範囲で二分探索する（ダウンロードはキャラ間で共有）。
  新しいコミットが無ければ API 1回で終わる。履歴の最初（2023-12-24）から載っているキャラはその日付。
- 昇格オペレーターは載らないので直近実装にならない。記録に無い非昇格の JP 実装済みオペレーターは
  `checked_at` より後に実装されたので、その日付で代用する（regen_seeds 前でも当日分が直近実装に入るように）。
- `customOperatorZhToJa.yaml` は名前の仮訳専用。`operator_data.rs` の
  `custom_operator_names_need_no_tidying` が、誤字のキー・JP 実装済みで消してよい行・仮訳の無い
  CN 限定オペレーターを見つけて落ちる（regen_seeds 後に落ちたら直す）。
