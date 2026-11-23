# astesiabot-rust

アークナイツ支援の Discord bot（poise/serenity）と web API・Web UI（axum）を1プロセスで動かす。
本番は Heroku（https://astesiabot.com）。

**詳しいことは各フォルダの CLAUDE.md にある**（そのフォルダのファイルを触ると読み込まれる）:
`src/bot/` / `src/api/` / `src/api/fk_kill_calculator/` /
`src/engine/external_source/` / `src/engine/fk_kill_calc/` / `src/engine/recruit/` /
`src/engine/operator_cost_calc/`。ここには全体に効くことだけを書く。

CLAUDE.md には、守ること（不変条件）と、どこから読むかだけを書く。ファイルごとの役割や関数名の一覧は
書かない（コードを検索すれば分かり、名前を変えると古くなる）。CLAUDE.md に書いたパスは
`tests/claude_md_paths.rs` が実在を確かめる。

## コマンド

```powershell
cargo build
cargo test
cargo fmt                        # 整形（CI は cargo fmt --check で落とす）
cargo run                        # bot + API（.env の環境変数が要る。下記）
cargo run --bin serve_web        # Web UI だけ（bot・外部取得なし、.env 不要。WEB_UI_PORT 既定3001、127.0.0.1のみ）
cargo run --bin regen_seeds      # data/seed/*.json を作り直す（週1で自動 PR が来るので、手で回すのは急ぐときだけ）
$env:UPDATE_GOLDEN=1; cargo test risei_golden; Remove-Item Env:UPDATE_GOLDEN  # Seed を作り直したら理性価値表の golden も出し直す
$env:INSTA_UPDATE="always"; cargo test; Remove-Item Env:INSTA_UPDATE  # bot の文面の snapshot（snapshots/*.snap）を出し直す。差分を見てから commit
cargo mutants --file <ファイル> -j 4 -o $env:TEMP -- --lib  # テストが変化に気づけない所を探す（手元専用・時間がかかる。リファクタリングの前に）
cargo run --bin regen_uranai_tools  # 占い館の data/uranai/toolList.yaml を作り直す
# 公開求人のローカル OCR のテンプレート作り直し（手元のスクショが要る。詳しくは src/engine/recruit/CLAUDE.md）
cargo run --release --bin cut_tag_boxes; cargo run --release --bin fit_tag_templates -- --export data/recruitment/tag_templates
# 本番の求人スクショ＋botのタグ返信を集める（手元専用。環境変数 COLLECT_DISCORD_TOKEN に本番トークン、.env は読まない。出力は gitignore 済みの local/ へ）
$env:COLLECT_DISCORD_TOKEN = heroku config:get DISCORD_TOKEN -a discordbot-riseicalculator  # 値を画面に出さずに渡す
cargo run --bin collect_recruit_samples -- --channel (heroku config:get CHANNEL_ID_KOUKAI_KYUJIN -a discordbot-riseicalculator) [--limit 500] [--max-pages 50] [--before <メッセージID>] [--out local/recruit_samples]
cargo run --release --bin cut_tag_boxes -- [--debug-dir local/recruit_samples/debug]  # 集めた画像からタグの箱を検出して切り抜く（手元専用）
cargo run --release --bin fit_tag_templates  # 箱とタグの対応を決めてテンプレートを作り、新しい20%で取り違え・全部読めた率を測る（手元専用）

# フロントエンドの検証（node は PATH に無いので絶対パスで）
& "C:\Program Files\nodejs\node.exe" src/api/lod_chest_solver/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/fk_kill_calculator/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/fk_kill_calculator/e2e.mjs
& "C:\Program Files\nodejs\node.exe" src/api/ef_recipe_calculator/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/ef_recipe_calculator/e2e.mjs
& "C:\Program Files\nodejs\node.exe" src/api/wl_battery_simulator/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/wl_battery_simulator/e2e.mjs
& "$env:LOCALAPPDATA\biome-2.6.6\biome-win32-x64.exe" lint   # JS の lint（設定は biome.json。CI は biome ci --error-on-warnings）
```

- 変更したら `cargo test`・`verify.mjs`（計算層。DOM 非依存のモジュールを import）・`e2e.mjs`
  （表現層。Playwright で実ブラウザ）を通し、必要なら実際に画面や Discord で触って確かめる。
- PR と master への push で CI（`.github/workflows/ci.yml`。Ubuntu で `cargo fmt --check`・clippy `-D warnings`・`cargo test`・Biome・
  verify.mjs）が回る。e2e.mjs は Windows 専用なので CI に無く、手元で回す。
- JS の lint は Biome（単体の実行ファイル。node_modules は要らない）。版は CI（`setup-biome` の version）と
  `biome.json` の `$schema` に揃え、手元は GitHub の公式リリースの `biome-win32-x64.exe` を上の場所に置く。
  整形（formatter）はかけない。見た目だけのルールは切ってあり、使っていない変数・import などを拾う。
- 依存の脆弱性は `.github/workflows/audit.yml`（`cargo audit`。PR・master・週1）で見る。必須チェックではない。
  赤くなったらまず `cargo update -p <crate>` で直るか試し、直せないものだけ `.cargo/audit.toml` に
  理由と外せる条件を書いて ignore する。
- 理性価値表は `risei_calculator_engine/testdata/golden_*.json` と突き合わせる。Seed を作り直すと
  golden のテストが落ちるので、diff で値の動きを確かめてから golden を出し直し、Seed と一緒に commit する。
  Seed の更新とコードの変更は同じ commit に混ぜない（コードの退行が Seed の変化に紛れる）。
- `e2e.mjs` は `serve_web` を自分で spawn して `/health` を待ち（初回ビルド込みで最大240秒）、
  最後に `taskkill /pid <pid> /T /F` で子プロセスごと落とす。Playwright はプロジェクト直下でなく
  `%LOCALAPPDATA%\npm-cache\_npx\<hash>\node_modules\playwright` にあり、`src/api/e2e_playwright.mjs` が
  自動で探す（版ごとに要るブラウザが違うので、Chromium が入っている版のうち一番新しいものを選ぶ）。
  どの版にもブラウザが無ければ `npx playwright install chromium`。
- 環境変数は `.env` に一覧がある（DISCORD_TOKEN、CLOUDVISION_API_KEY、CHANNEL_ID_*、FK_SHEETS_*、
  YouTube 関係など）。足りないと起動時に panic する（YouTube のお知らせだけは任意で、無ければ起動しない）。
- 動作確認の URL: `/recruitment/`（POST）、`/WLBatterySimulator`、`/EFRecipeCalculator`、
  `/LodChestSolver`（`/en`）、`/FrameKillCalculator`（`/catalog.json`）、
  `/api/risei`、`/robots.txt`、`/sitemap.xml`。

## Windows での注意

- askama はテンプレートをバイナリに埋め込む。`templates/*.html` の変更は**再ビルド＋再起動**が要る
  （`static/*` はリロードで反映）。サーバーを起動したまま `cargo build` すると exe のロックで失敗する（os error 5）。
- `src/bin/` の実行ファイル名に `update` / `install` / `setup` / `patch` 等を含めない。UAC のインストーラー
  検出に引っかかり管理者権限を求められる（os error 740）。
- `data/` はカレントディレクトリ基準で読むので、プロジェクトルートで実行する。
- 型エラーが出やすかった所: poise 0.6 / serenity の embed API、`event_handler` のクロージャ型と
  `FrameworkContext`、`fancy_regex` の `find` が `Result<Option<Match>>`。YAML は `serde-saphyr`（`serde_yaml` ではない）。
  バージョンが原因なら Cargo.toml を調整してよい。

## 構成

```
src/
├── lib.rs / main.rs   … main.rs が ExternalSourceRegistry・RecruitEngine・RiseiCalculatorEngine を
│                        起動時に作り、bot と api に Arc で共有。日次の refresh_all と理性価値表の再計算ループも起動
├── bin/               … regen_seeds / regen_uranai_tools / serve_web（main.rs に依存しない）
├── engine/            … ドメインロジック。bot にも api にも依存しない
│   ├── external_source/      … 外部データ（ゲームデータ・penguin-stats・FK シート）の取得とキャッシュ
│   ├── risei_calculator_engine/ … 理性価値表の計算・ステージ検索・課金パック効率
│   ├── operator_cost_calc/   … 育成コスト（昇進・特化・モジュール）とランキング
│   ├── recruit/              … 公開求人のタグ抽出・計算・整形（tag_ocr/ はローカルのタグ枠 OCR）
│   ├── fk_data_search/       … FK 情報の検索
│   └── fk_kill_calc/         … フレームキル計算機のカタログ組み立て
├── api/               … axum。/recruitment/、/api/risei/*、各 Web UI ツール
└── bot/               … Discord bot。コマンドとチャンネルごとのサービス
data/                  … 実行時に読む静的データ（recruitment/ risei/ uranai/ fk_kill_calc/ seed/ と名前の仮訳 YAML）
assets/icon/           … サイトアイコンの元画像と generate.mjs
```

依存の向き: `external_source` は何にも依存しない → 他の engine はその DTO に依存する純粋ロジック →
bot と api が engine に依存する。だから同じ計算を Discord でも web でも使える。

## 全体のルール（壊さないこと）

- **設計先行**: 一定以上の規模の変更は、まず設計案（DTO・責務の置き場所・影響ファイル）を出して
  レビューを受け、承認されてから実装する。
- **計算と表示を分ける**: 計算関数は DTO を返し、整形（Discord embed、AI 向け文字列、JSON 応答）は
  呼び出し側に置く。
- **リファクタリングでは挙動を変えない**: 既存のテスト・golden・snapshot は書き換えない（足すのはよい）。
  書き換えが要るなら挙動が変わっているので、リファクタリングと分けて理由を書く。整形（`cargo fmt`）・
  `cargo clippy --fix`・コードの移動だけの変更は、ロジックの変更と別の commit にする。触る前に
  `cargo mutants` でその範囲を調べ、生き残った変異（壊してもテストが落ちない所）にテストを足してから始める。
  bot の文面の snapshot は手で作った DTO から作る（Seed を使うと週1の Seed の PR が毎回落ちる）。
- **挙動はコード、意図はテストで守る**: コメントには、コードから読み取れない「なぜ」だけを書く。
  中で完結して結果を確かめられる約束（並び順・分割位置・丸め方など）はコメントでなくテストにする。
  外の制約（Heroku・Discord の API・OCR のクセなど）と、速さ・設計の判断はコメントに残す。
  経緯（以前は〜、日付、issue・PR 番号）は commit と PR に書き、コメントには書かない
  （`tests/history_comments.rs` が日付・番号・「以前は」などを見つけると落ちる）。
  他のファイルの挙動を説明するコメントも、相手を直すと古くなるので書かない。
- **外部データの fetch 失敗**: 起動時に失敗 → Seed があれば使う、無ければ panic。起動後の再 fetch に
  失敗 → 直前の値のまま続ける（panic しない）。この非対称を崩さない。fetch は `external_source/http.rs`
  の共通関数（7s タイムアウト・最大3回リトライ）を使う。
- **実行時にファイルを書かない**: Heroku はファイル書き込みが再起動・再デプロイで消える。Seed は
  起動時に読むだけで、更新は手元で `regen_seeds` → commit。状態を残したい機能も別の手段を取る
  （例: YouTube のお知らせは Discord の履歴で二重投稿を判定）。
- **理性価値表は更新と参照を分ける**: `RiseiCalculatorEngine` の参照系（`snapshot()`・検索・lists）は
  メモリ上の計算済みキャッシュを読むだけ。更新は main.rs のループが `REFRESH_INTERVAL`（120分）ごとに
  `refresh()`（成功したものだけ差し替え）。リクエスト時には再計算しない（応答を遅くしないため）。
  `cargo run -- --debug` は外部 fetch を一切せず Seed だけで動き、更新ループも動かさない。
- **Web UI のルートは `api::web_ui_router()` に足す**（`run_api` と `serve_web` でルートがずれないように）。
  絶対 URL は `api::base_url()` から作る。詳細は `src/api/CLAUDE.md`。
