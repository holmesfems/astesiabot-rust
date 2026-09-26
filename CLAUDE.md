# astesiabot-rust

Discord bot（poise/serenity）と web API（axum）を1プロセスで並行稼働させる
アークナイツ支援 bot の Rust 移植プロジェクト。元は Python 実装。

## ビルド

`cargo build` は通る状態。型エラーに遭遇したら、過去に出やすかったのは以下:

- poise 0.6 / serenity の embed API（`CreateEmbed`, `CreateMessage`,
  `.embeds()`, `.colour()`）のシグネチャ差異
- `event_handler` のクロージャ型と `FrameworkContext` の扱い
- `fancy_regex::Regex` の `find` が `Result<Option<Match>>` を返す点
- `serde_yaml` のバージョン差異

バージョンが原因なら Cargo.toml のバージョンを調整してよい。
API の使い方が変わっている場合は各クレートの最新ドキュメントに合わせて修正。

`src/bin/` に新しい単独ツールを足すとき: Windows では実行ファイル名に
`update`/`install`/`setup`/`patch` 等を含めると UAC のインストーラー検出
ヒューリスティックに引っかかり、実行に管理者権限が要求される（os error 740）。
`regen_seeds` のような紛らわしくない名前にすること。

## アーキテクチャ

```
src/
├── lib.rs         … astesiabot_rust ライブラリクレート本体（pub mod api/bot/engine）。
│                    main.rs と src/bin/*.rs の両方がここに依存する
├── main.rs        … bot 本体のエントリポイント。RecruitEngine / ExternalSourceRegistry を
│                    起動時ロード → bot と api に Arc で共有。1日1回
│                    ExternalSourceRegistry::refresh_all() を叩くループも起動
├── bin/
│   ├── regen_seeds.rs … external_source の Seed（data/seed/*.json）を手動再生成する
│   │                      独立ツール。main.rs には依存しない。使い方は後述
│   └── serve_web.rs   … Web UI（AppStateに依存しないページ群）だけを配信するdevサーバー。
│                          bot も ExternalSourceRegistry も起こさず、.env も要求しない
│                          （`WEB_UI_PORT`、既定3001。bindは127.0.0.1のみ）。
│                          `api::web_ui_router()` を使うので run_api とルートは常に一致する。
│                          `src/api/test_runner/e2e.mjs` がこれをspawnして使う
├── engine/
│   ├── external_source/ … 外部サイトから取得する情報のレジストリ（bot にも api にも
│   │   │                依存しない）。起動時に一括fetchしてメモリ保持し、以後は
│   │   │                機能側がそこから参照する（例: birthday.rs が operator_data を参照）
│   │   ├── mod.rs        … ExternalSourceRegistry。load / refresh_all（定期実行用の一括fetch）/
│   │   │                    refresh_by_name（機能側からの個別オンデマンド更新用）。
│   │   │                    SEED_JOBS（regen_seeds が使うSeed生成ジョブ一覧）もここ。
│   │   │                    情報源を増やす手順はこのファイルのコメントを参照
│   │   ├── cache.rs      … Source<T>。fetch結果をメモリ保持しつつ、任意でSeed（fetch失敗時の
│   │   │                    代替用JSONファイル）を読む。起動時fetch失敗→Seedがあれば使用、
│   │   │                    無ければpanic。起動後の再fetch失敗→直前のメモリを保持したまま継続。
│   │   │                    Seedの書き込み（write_seed_file）は実行時には呼ばない
│   │   │                    （regen_seeds からのみ使う。理由は下記ポイント参照）
│   │   ├── http.rs       … 全情報源共通のfetch戦略（7sタイムアウト・最大3回リトライ）
│   │   ├── fk_data.rs    … FK情報スプレッドシート(Google Sheets API v4)の生データ。
│   │   │                    スキル名解決はせず行データ(オペレーター名→行一覧)のみ保持。
│   │   │                    FK_SHEETS_API_KEY/FK_SHEETS_SPREADSHEET_ID(.env)を使用。
│   │   │                    ExternalSourceRegistry::refresh_all（日次バッチ）には含めない
│   │   │                    （engine/fk_data_search が自前の1時間TTLで読み取り駆動更新するため）。
│   │   │                    SEED_PATH = data/seed/fk_data.json
│   │   ├── operator_data.rs … オペレーターCN→JA名変換 +
│   │   │                       昇進/スキル特化/モジュール消費素材の生データ。character_table.json /
│   │   │                       uniequip_table.json / char_patch_table.json をまとめて1回のfetchで
│   │   │                       構築する（旧operator_names.rsはこれに統合済み）。
│   │   │                       SEED_PATH = data/seed/operator_data.json
│   │   ├── operator_combat.rs … フレームキル計算機用のオペレーター戦闘生データ
│   │   │                       （元ATK/信頼度込みATK・潜在ATK・モジュールのATK加算値。
│   │   │                       machine-extractableな数値のみ）。character_table.json /
│   │   │                       uniequip_table.json / battle_equip_table.json から構築する。
│   │   │                       operator_data.rs（消費素材ドメイン）とは意図的に別ソース
│   │   │                       （オーナー方針: 両ドメインを混ぜない。詳細は下記ポイント参照）。
│   │   │                       SEED_PATH = data/seed/operator_combat.json
│   │   └── skill_data/      … スキルID→表示名+説明文+blackboard（skill_table.json をfetch）。
│   │       ├── mod.rs          … SkillData（旧skill_names.rsのSkillNamesを統合）。get_str/
│   │       │                      get_description/get_blackboard（最大レベルのblackboard。
│   │       │                      フレームキル計算機のスキル倍率取得に使う）。
│   │       │                      SEED_PATH = data/seed/skill_data.json
│   │       ├── raw.rs          … skill_table.jsonの生JSON構造体
│   │       └── description.rs  … 最大レベルの説明文組み立て（タグ除去・プレースホルダ解決・
│   │                              ヘッダ合成）。Python版のcleanStr副作用バグ（フォーマット指定子の
│   │                              小数点が壊れる/chain.max_targetの誤統合）は踏襲せず、実データに
│   │                              即して正しく表示する（詳細はファイル冒頭コメント参照）
│   ├── operator_cost_calc/ … ★オペレーター消費素材ドメインの純粋ロジック+DTO（bot にも api にも
│   │   │                      依存しない。Python charmaterials.py 相当）
│   │   ├── mod.rs     … AllOperatorsInfo（検索・ランキング）、ValueSet（グローバル/大陸版の
│   │   │                RiseiValuesを束ねる）、SkillCostInfo
│   │   ├── model.rs   … ItemCost（アイテムID→個数。挿入順保持のIndexMapで、Pythonのdict
│   │   │                挿入順セマンティクスを再現。タイ項目の並び順一致に必須）、FormulaMap
│   │   ├── aggregate.rs … OperatorCosts相当の集計（totalPhaseCost/totalSkillMasterCost等）
│   │   ├── dto.rs     … 4コマンド分のDTO（ItemCostView等）。整形はしない
│   │   └── calc.rs    … DTOを返す計算関数（skill_master_cost/operator_elite_cost/
│   │                     operator_module_cost/cost_list_*）
│   ├── recruit/   … ★求人ドメインの純粋ロジック（bot にも api にも依存しない）
│   │   ├── mod.rs     … RecruitEngine。process_from_ocr（API用）/ process_for_embed（bot用）
│   │   ├── model.rs   … Operator, Tag, TagType
│   │   ├── calc.rs    … タグ計算エンジン（ピックアップ対応、future オペレーター実装済み）
│   │   ├── matcher.rs … OCR生テキスト → タグ抽出（fancy-regex、3言語辞書＋誤字補正）
│   │   └── format.rs  … 出力整形（display_chunks / response_for_ai / make_title / 分割）
│   ├── fk_data_search/ … ★FK情報検索ドメインのDTO+検索ロジック（bot にも api にも依存しない。
│   │   │                  Python fkDatabase/fkDataSearch.py 相当）
│   │   ├── mod.rs     … FkDataSearchEngine（external_source::fk_data の1時間TTL読み取り駆動更新。
│   │   │                daily refresh_allとは別軸）、FkDataView（fk_data+operator_data+skill_data
│   │   │                のスナップショットを束ねてsearch/autocompleteを提供）
│   │   ├── dto.rs     … FkSearchResult（OperatorNotFound/NeedsSkillSelection/SkillNotFound/Found）
│   │   └── search.rs  … resolve（オペレーター名+スキル指定→FkSearchResult）、autocomplete。
│   │                    skill_id_by_num（skill_num→skillId解決）は`pub(crate)`で
│   │                    fk_kill_calc からも再利用する（重複させない）
│   └── fk_kill_calc/  … ★フレームキル計算機の「機械データ+手動補正」マージ層（bot にも api にも
│       │                  依存しない）。3層構成: 1.機械データ(operator_combat+skill_dataの
│       │                  blackboard) 2.手動補正(overrides.rs↔data/fk_kill_calc/overrides.yaml)
│       │                  3.マージ(build_catalog)。詳細は下記ポイント参照
│       ├── mod.rs     … build_catalog（fk_data起点でoperator_combat/operator_data/skill_data/
│       │                overridesをマージしCatalogBuildを返す。名前解決できなかった
│       │                fk_data上のオペレーター名は`skipped`に集約する）、
│       │                resolve_multiplier_defaults（倍率の自動判定+候補一覧の決定的な並び順）、
│       │                validate_overrides（overrides.yamlのドリフト検知）
│       ├── dto.rs     … Catalog/CatalogOperator/FkEntry等（JSONはcamelCase）。
│       │                Valued<T>{value,source}でAuto/Manualの出所を持つ
│       ├── tags.rs    … profession/positionからタグ・ダメージ属性の初期値を推測する
│       └── overrides.rs … data/fk_kill_calc/overrides.yaml（include_str!でビルド時埋め込み。
│                           実行時ファイルI/Oなし）のロード。`Overrides::global()`でプロセス内
│                           1回だけパースして使い回す
├── api/
│   ├── mod.rs             … axum。AppState、run_api、base_url()（canonical/hreflang/sitemap用の
│   │                        絶対URL起点）、/robots.txt・/sitemap.xml。`web_ui_router()`
│   │                        （AppStateに依存しないUIルートだけを束ねる公開関数）もここ。
│   │                        UIのルートはここに足せば run_api にも serve_web にも自動で
│   │                        反映される。片方にだけ書かないこと
│   ├── recruitment.rs     … POST /recruitment/ （Python の doRecruitment と完全一致）
│   ├── legacy_host_redirect.rs … 旧ホスト（*.herokuapp.com / www.）の GET/HEAD を
│   │                        PUBLIC_BASE_URL へ 301 するミドルウェア（run_api のみに掛ける）
│   ├── site_icons/        … サイト共通アイコンをルート直下で配信（/favicon.ico・/favicon.svg・
│   │                        /apple-touch-icon.png・/icon-{192,512,maskable-512}.png・
│   │                        /site.webmanifest）。include_bytes! で埋め込み。static/ の画像は
│   │                        `assets/icon/generate.mjs` の生成物（手で編集しない）
│   ├── templates_shared/  … 各ページが include する共通パーツ。toolnav.html（ツール切り替え）/
│   │                        head_icons.html（全ページの<head>に必須）/ og_square_image.html
│   │                        （横長OGPを持たないページ用の正方形 og:image。home 以外の全ページが
│   │                        使う。include先のstructに base が必要）
│   ├── wl_battery_simulator/ … 武陵発電制御シミュレーター（askama + htmx の Web UI）
│   │   ├── mod.rs        … ルーター（index/calculate/static配信）
│   │   ├── battery_sim.rs… シミュレーションエンジン（Python版 batterySim.py 移植）
│   │   ├── optimizer.rs  … 図面ごとの最適化（Python版 optimizer.py 移植。最大発電量超は按分計算）
│   │   ├── templates/    … index/result/chart/error.html
│   │   └── static/       … css/画像/tutorial html
│   ├── ef_recipe_calculator/ … エンドフィールド レシピ計算機（askama + htmx の Web UI）
│   │   ├── mod.rs        … ルーター。フォームは RecipeSet + CalcRequest を単一 payload(JSON文字列)で受ける
│   │   ├── solver.rs     … 計算エンジン（純粋関数。RecipeSet + CalcRequest -> CalcResult。
│   │   │                   アルゴリズム詳細は EFRecipeCalculator.md §4 参照）
│   │   ├── templates/    … ef_index.html ほかフラグメント（ef_* 前置。理由は下記ポイント参照）
│   │   └── static/       … app.js / style.css / presets.json
│   ├── lod_chest_solver/  … 幽霊船 宝箱ソルバー（レジェンド オブ ドラグーン。アークナイツ外の単発ツール）
│   │   ├── mod.rs        … ルーター。"/"=ja / "/en"=en / "/static"=ServeDir。
│   │   │                   言語ごとに別URL・別HTML（1URL=1言語。理由は下記ポイント参照）
│   │   ├── templates/    … lod_index.html（ja）/ lod_index_en.html（en）。
│   │   │                   静的ラベルはmarkupに直書き＋SEOタグ（canonical/hreflang/OG/JSON-LD）
│   │   └── static/       … engine.js（純粋ソルバー。DOM非依存）/ ui.js（DOM描画。文言は
│   │                       initUi(strings)で各ページから受け取る）/ style.css（言語共通）
│   ├── test_runner/       … 試験手順ランナー（元は test-procedure/test_runner.html。
│   │   │                    手順書のMarkdownを読み込みOK/NGを押すだけで進められる。
│   │   │                    CDN参照ゼロ、進捗はサーバーに送らない。アークナイツ外の単発ツール）
│   │   ├── mod.rs        … ルーター。"/"=ja / "/en"=en（lod_chest_solverと同じ1URL=1言語）。
│   │   │                   "/static"=ServeDir。JS/CSSは全てここから配信する。
│   │   │                   "/skill.zip"=test-procedure-formatterスキルの配布zip
│   │   │                   （リクエスト毎に組み立てる。詳細は下記ポイント参照）
│   │   ├── skill/        … 手順書整形AIエージェント用スキル test-procedure-formatter の正本
│   │   │                   （SKILL.md / format.ja.md / format.en.md / validate.mjs の4ファイル。
│   │   │                   parser.jsは置かない。理由は下記ポイント参照）。`/TestRunner/skill.zip`
│   │   │                   がここ+static/js/core/parser.jsから毎回zipを組み立てて配布する
│   │   ├── verify.mjs    … static/js/core/ と constants/ の検証（実モジュールをimportして
│   │   │                   実行）。ui/ はDOM依存なので対象外。実行方法は「動作確認手順」参照
│   │   ├── e2e.mjs       … 表現層のブラウザ実機テスト（Playwright）。`cargo run --quiet
│   │   │                   --bin serve_web` を自分でspawnし、`/health` を待ってから
│   │   │                   ja/enの両方を検証して必ず後始末する。verify.mjs と対になる
│   │   │                   ものなので、DOM操作が絡む変更（画面遷移・演出・ボタンid等）は
│   │   │                   こちらで検証する。実行方法は「動作確認手順」参照
│   │   ├── static/
│   │   │   ├── lz-string.min.js … 1.5.0, MIT。進捗URL共有 `#state=<圧縮JSON>` の圧縮/解凍用。
│   │   │   │                      CDN参照せず同一オリジン配信
│   │   │   ├── style.css        … ja/en 共通（言語差は文言だけなのでCSSは1本）
│   │   │   └── js/              … ES module。依存の向きは constants ← core ← ui ← main
│   │   │       ├── main.js      … boot({strings,phrases,samples}) と init() だけ
│   │   │       ├── constants/   … config.js（非文言の定数）/ i18n.js（T・P・S の器と
│   │   │       │                  installI18n）/ {strings,phrases,samples}.{ja,en}.js
│   │   │       ├── core/        … parser.js（Markdown→手順書データ）/ score.js /
│   │   │       │                  state.js（状態と集計）/ io.js（進捗JSON・共有URL・CSV等）。
│   │   │       │                  ★DOM非依存。documentを触らせないこと（verify.mjsが落ちる）
│   │   │       └── ui/          … dom.js（el/showScreen/toast/clipboard。ui内の最下層）/
│   │   │                          renderer.js / modal.js / flow.js /
│   │   │                          effects/{confetti,dodge}.js
│   │   └── templates/    … tr_index.html（ja）/ tr_index_en.html（en）。
│   │                       静的ラベルはmarkupに直書き＋SEOタグ。末尾は
│   │                       `<link>` と5行のブートストラップ（言語別constantsをimportして
│   │                       boot()を呼ぶ）だけで、ロジックもCSSも持たない。
│   │                       ツール切り替えバー（templates_shared/toolnav.html）は
│   │                       業務で使う想定のため意図的に include しない（バー側の
│   │                       TestRunnerチップは残す）
│   └── fk_kill_calculator/ … フレームキル計算機（`/FrameKillCalculator`）。日本語専用（/en無し）
│       ├── mod.rs        … `page_router()`（ページ本体+`/static`=ServeDir。`web_ui_router()`に
│       │                    足す）と`catalog_router(provider)`（`GET .../catalog.json`）を分離。
│       │                    catalog.jsonだけ別軸な理由・`run_api`/`serve_web`それぞれの
│       │                    providerの組み立ては下記ポイント参照
│       ├── templates/    … fkc_index.html（fkc_ 前置）。toolnav/head_icons/og_square_imageを
│       │                   include。SEOタグはbase_url()から組み立てる（lodと同じ方式）
│       ├── verify.mjs    … static/engine.js（DOM非依存）を直接importして検証。
│       │                   実行方法は「動作確認手順」参照
│       ├── e2e.mjs       … 表現層のブラウザ実機テスト（Playwright）。test_runner/e2e.mjsと
│       │                   同じ枠組み（serve_webをspawn、/healthを待つ、後始末は必ず実行）。
│       │                   実行方法は「動作確認手順」参照
│       └── static/
│           ├── engine.js       … 計算層。DOM非依存（`document`/`window`を参照しない）。
│           │                     atk/final/perHit/rowDamage計算、撃破提案(suggest)、
│           │                     stale row除去(dropStaleRows)。単位の約束はファイル冒頭コメント参照
│           ├── ui.js           … 表現層。カタログfetch・状態管理・DOM描画・URL(#state=...)の
│           │                     読み書き。再描画は状態変更のたびに#app配下を丸ごと作り直す方式
│           │                     （フォーカス位置は`withPreservedFocus`で復元する）
│           ├── style.css       … 420px想定の縦長1カラム。他ページ(home/lod_chest_solver)と
│           │                     同じくダーク固定（配色トークンはhome_index.htmlの:rootを流用）
│           └── lz-string.min.js … test_runner/static/lz-string.min.jsと同じ1.5.0, MITを
│                                   ここにも独立してvendoring（ツール間を疎結合に保つため、
│                                   test_runnerのURLは参照しない）
└── bot/
    ├── mod.rs     … run_bot(token, state)。setup() で ChannelRouting::from_env()・
    │                誕生日チャンネルの解決（未設定ならここでpanic）と誕生日スケジューラの spawn
    ├── data.rs    … Data { state, channel_routing: handler::ChannelRouting }
    ├── handler.rs … 汎用メッセージハンドラ（自分→スパム→bot→チャンネル別 の順で振り分け）。
    │                ChannelRouting（振り分け先チャンネルの定義・env解決）もここに同居。
    │                振り分け先を増やす時はここだけ触ればよい（data.rs/mod.rs は不変）
    ├── utils.rs   … channel_id_env(key)。各サービスの from_env() 相当から共通利用
    ├── commands/  … スラッシュコマンド 1コマンド1ファイル
    │   ├── ping.rs / echo.rs / add.rs
    │   ├── fkdatasearch.rs       … FK情報検索（/fksearch）。engine/fk_data_search を整形して
    │   │                           embed化。オートコンプリートはTTLチェックなしでfk_dataを直読み
    │   │                           （Python版autoCompleteの非対称性を踏襲）
    │   ├── risei/                … 理性価値計算コマンド群（riseimaterials等）
    │   └── operator_cost_calc/    … オペレーター消費素材コマンド群（Python charmaterials.py相当）
    │       ├── mod.rs             … build_context（AllOperatorsInfo+ValueSet構築）、
    │       │                        send_reply、fmt_item_block等の整形共通部。
    │       │                        golden_tests（下記参照）もここ
    │       ├── operatormastercost.rs … スキル特化消費素材（skillMasterCost）
    │       ├── operatorelitecost.rs  … 昇進消費素材（operatorEliteCost）
    │       ├── operatormodulecost.rs … モジュール消費素材（operatorModuleCost）
    │       └── operatorcostlist.rs   … 各種ランキング/統計（operatorCostList、8バリアント）
    └── services/
        ├── moderation.rs  … スパム検知・連投/爆撃対応・罠チャンネル自動削除・全体通知BAN（実装済み）
        ├── anniversary.rs … 周年ロール付与など
        ├── birthday.rs    … オペレーター誕生日自動お祝い（毎日 JST 0:00、operator_data を利用）
        ├── uranai.rs      … 占い館（骨組みのみ、未実装。ChatGPT API連携が必要）
        └── koukai_kyujin/ … 公開求人
            ├── mod.rs … OCR → process_for_embed → embed 送信
            └── ocr.rs … Google Vision REST（v1〜v1p4beta1 を切替。連続失敗対策）

data/  … 実行時に読み込む（カレントディレクトリ基準なのでプロジェクトルートで実行）
├── recruitment/               … 求人ドメイン（engine/recruit が読む）
│   ├── operators.json         … オペレーターDB（main 153体 + future プール）
│   ├── tagList.json           … タグ種別定義
│   └── tagJaToJa.yaml / tagEnToJa.yaml / tagZhToJa.yaml … 3言語辞書
├── birthdayRev.yaml           … 日付→誕生日オペレーター(中国語名)一覧
├── customOperatorZhToJa.yaml  … オペレーターCN→JA名前フォールバック（JP未実装オペレーター用の仮訳）
├── customItemId.yaml          … 理性価値計算で使う特殊アイテムのID補完（例外用の予備ファイル）
├── customItemZhToJa.yaml      … アイテムCN→JA名前フォールバック（customOperatorZhToJaのアイテム版）
├── fk_kill_calc/
│   └── overrides.yaml     … フレームキル計算機の手動補正データ（operator_id→skill_num→
│                             バリアント一覧）。`include_str!`でビルド時埋め込み。
│                             スキーマ・記入例はファイル冒頭コメント参照
├── golden/operator_cost_calc/ … Python版charmaterials.pyの出力をゴールデンJSON化したもの。
│                                 `ref_python/.../dump_charmaterials_golden.py`で生成し、
│                                 bot/commands/operator_cost_calc の golden_tests が実ネットワーク
│                                 テスト(#[ignore])で突き合わせる。詳細は下記ポイント参照
└── seed/                      … external_source の Seed（`cargo run --bin regen_seeds` で
                                   生成し、git commitして含めておく。詳細は下記ポイント参照）
    ├── operator_data.json
    ├── operator_names.json    … 旧operator_names.rsが残したSeed。operator_data統合後は
    │                             regen_seedsでは更新しない。名前解決が壊れていないかの
    │                             ゴールデン参照として意図的に残置している
    ├── operator_combat.json
    ├── skill_data.json
    └── fk_data.json
```

依存方向: recruit / external_source は何にも依存しない純粋ロジック。bot と api が
これらに依存する。これにより求人計算ロジックや外部情報を bot でも web API でも共有できる。

## 設計上の重要ポイント（壊さないこと）

- **Python 完全一致が要件**。calc.rs / format.rs / matcher.rs のロジックは
  Python 版と1文字レベルで出力一致するよう移植済み。挙動を変えないこと。
- **文字数 vs バイト数**: format.rs の分割処理は Python の len()（文字数）に
  合わせて `chars().count()` を使っている。`String::len()`（バイト数）に
  変えると日本語で分割位置がズレるので変えない。
- **安定ソートの重ねがけ**: format.rs の sort_items は Python の sorted 重ねがけ
  を再現している。順序を変えると結果が変わる。
- **star_set は BTreeSet**: 出力の星表記（★4,5 など）を昇順で安定させるため。
  HashSet に変えない。
- **fancy-regex を使う理由**: matcher.rs の `(?!上級)`（否定先読み）が標準 regex
  では書けないため。標準 regex に置き換えないこと。
- **external_source の fetch 失敗ポリシー**: 起動時fetch失敗 → Seedがあれば使用、
  無ければ panic。起動後の再fetch失敗 → 直前のメモリを保持して継続（panicしない）。
  この非対称性（起動時はpanicし得る／再fetchはしない）を崩さないこと。
- **Seedは実行時に書き込まない**: Heroku 等は実行時のファイル書き込みが dyno
  再起動・再デプロイで揮発するため、`Source` は起動時に Seed を**読む**だけで、
  fetch成功時に書き戻すことはしない。Seedの更新は `cargo run --bin regen_seeds`
  （main.rs 非依存の独立ツール。`engine/external_source/mod.rs` の `SEED_JOBS` を
  順に実行して `data/seed/*.json` を書き換える）を手元で実行し、差分を
  `git commit`/`push` してリポジトリに含める運用。push前に思い出したタイミングで
  都度実行すればよい（自動化はしていない）。
- **fetchの共通戦略**: `engine/external_source/http.rs` の `client()` /
  `fetch_json_with_retry()`（7sタイムアウト・最大3回リトライ）が全情報源共通。
  新しい情報源を足すときもこれを使い、fetch fn ごとに個別のタイムアウト/リトライ
  ロジックを実装しないこと。
- **責務分離の意識**: 計算層と表現層を分ける。計算関数はDTOを返し、整形（Discord Embed / AI向け文字列など）は各呼び出し側に置く。pythonの設計（例: RCReplyが両出力を1型に詰める形）は踏襲せず、適切な形を優先して良い。
- **設計先行**: 一定以上の規模の変更は、まず設計案（DTO・責務配置・影響ファイル）を提示し、実装前にレビューを挟む。承認を得てから実装に入る。
- **operator_cost_calc の ItemCost は挿入順保持が必須**: `engine/operator_cost_calc/model.rs` の
  `ItemCost` は内部表現に `IndexMap` を使い、`Cargo.toml` で `serde_json` の `preserve_order` を
  有効にしている。Python の dict は挿入順を保持し、`normalize()`（表示直前の並び替え）は
  value_target に無い項目（SoC芯片等）を「その時点の挿入順」で安定ソートするため、これを
  崩すと表示順がPython版と食い違う（実例: モジュール消費のデータ補完チップ/マシンが複数
  種類同時に出るケース）。`HashMap`や`BTreeMap`に戻さないこと。
- **skill_data の説明文組み立てはPython版と意図的に異なる**: `engine/external_source/skill_data/description.rs`
  はPython `SkillIdToName.SkillItem`のblackboard置換を再現するが、Python版の`cleanStr`副作用
  （数値フォーマット指定子の小数点を巻き込んで壊す/`chain.max_target`を`max_target`へ誤統合する）
  は踏襲せず実データに即して正しく表示する。理由の詳細は`description.rs`冒頭コメント参照。
- **skill_table.json の`spData.spType`は数値が混じる**: CNデータの一部スキル(PASSIVE中心に
  600件超、2024年時点で確認済み。例: イネスのスキル3)は`spType`が文字列でなく数値(`8`等)に
  なっている。`engine/external_source/skill_data/raw.rs`の`RawSpData::sp_type`は`string_or_number`
  という独自deserializerで両方を受け付ける。ここを`String`型のまま厳格にderiveすると、該当スキル
  1件全体（name/descriptionを含む）がdeserialize失敗で丸ごと`SkillData`から欠落し、
  `SkillData::get_str`が"Missing"を返す（Python版はduck typingのため欠落しない）。`String`型に
  戻さないこと。
- **operator_cost_calc は説明文をembedに表示していない**: `SkillData::get_description`は用意済みだが、
  skillMasterCostのembedへの表示はまだ配線していない（スコープ外）。表示したくなったら
  `bot/commands/operator_cost_calc/operatormastercost.rs`から呼べばよい。
- **fk_data の1時間TTLは日次refresh_allと別軸**: `engine/external_source/fk_data.rs`は
  `ExternalSourceRegistry::refresh_all`（日次バッチ）の対象に含めていない。代わりに
  `engine/fk_data_search::FkDataSearchEngine`が読み取り駆動（コマンド呼び出し時に前回チェックから
  1時間経過していれば再fetch）でTTL管理する（Python `FKInfo.getInfoFromName`のポーリング方式を踏襲）。
  オートコンプリート（`bot/commands/fkdatasearch.rs`の`autocomplete_operator_name`）はこのTTL
  チェックを経由せず`external_source.fk_data`を直接読む（Python `FKInfo.autoComplete`と同じ非対称性）。
- **operator_cost_calc のゴールデンテストは理性価値に許容誤差を持つ**: risei_calculator_engine
  の基準マップ選定は乱数を使うため（近接タイの複数カテゴリが実行毎に異なる基準ステージへ
  収束し得る。Python版も`random.choice`で同様）、理性価値はPython版と実行毎に僅かに
  （観測上0.02未満）ズレる。`bot/commands/operator_cost_calc/mod.rs`の`golden_tests`は
  数値のみ許容誤差付きで比較し、近接タイによる隣接2件の順序入れ替えも許容する
  （`lines_match_with_adjacent_swap_tolerance`）。ゲームデータ更新でランキング内容自体が
  変わった場合は`ref_python/RiseiCalculatorBot-main/dump_charmaterials_golden.py`を再実行して
  `data/golden/operator_cost_calc/*.json`を更新すること（`regen_seeds`もセットで実行）。
- **askama.toml の dirs はフラットに解決される**: テンプレートは登録済みディレクトリ横断で
  ファイル名だけで引かれるため、名前が衝突すると解決が曖昧になる。モジュールごとに
  前置する（`ef_*` / `lod_*` / `tr_*`）。`index.html` のような素の名前は wl_battery_simulator の
  ものと衝突するので新規モジュールでは使わない。
- **lod_chest_solver / test_runner は言語ごとに別URL・別HTML**: 実行時にJSで文言を差し替える
  方式（localStorage / navigator.language での判定）は採らない。クローラに両言語の中身を
  見せるのが目的なので、静的ラベルは各言語テンプレートのmarkupに直書きし、動的に
  組み立てる文言だけを渡す。Accept-Language による自動振り分けもしない（1URL=1言語を
  崩すとクローラ側で重複扱いされ得る）。
  どちらも計算層と表現層を言語間で共有し、言語差は文言データだけに閉じ込める。
  lod_chest_solver は `static/engine.js`（DOM非依存）+ `static/ui.js`（文言は `initUi(strings)`
  で受け取る）。test_runner は `static/js/` のESモジュール群 + `constants/*.{ja,en}.js`
  （テンプレートが自分の言語のものをimportして `boot()` に渡す）。
  CSSも `static/style.css` 1本を両言語で共有する。
- **test_runner の core/ はDOM非依存を維持すること**: `static/js/core/` の4モジュールは
  `document` を一切参照しない。そのおかげで `verify.mjs` が実モジュールをimportして
  node（VS CodeのElectron）だけで検証できる。`window`/`localStorage`/`LZString` は
  `state.js` の保存3関数と `io.js` の共有URLに限って触ってよいが、`document` は不可。
  `constants/config.js` にも実行時に `window` を評価する値を置かないこと（`core/` が
  importしているので、置くと `verify.mjs` が動かなくなる。環境フラグは `ui/dom.js` へ）。
  `ui/` 側は renderer ↔ modal ↔ flow ↔ effects/confetti が相互依存している（画面遷移の
  相互呼び出し）。ESMは関数の循環importを正しく扱うので許容しているが、**importした
  ものをモジュールのトップレベルで呼ばないこと**（TDZで落ちる）。`ui/dom.js` は
  循環に入っていない葉なので、ここにアプリ層のフローを足さないこと。
- **test_runner のパーサーは日英両方のキーワードを受け付ける**:
  `用語定義|Definitions?|Glossary` / `配布物|Attachments?|Downloads?|Assets?` /
  `ビルド|Build|Version` / ビルド値の記入モード `記入|入力|Enter|Fill ?in|TBD`。
  エイリアスを足すときは **`(?:...)` の非キャプチャグループ**を使うこと
  （`BUILD_LINE_RE` の `m[1]` と `BUILD_INPUT_RE` のグループ1を呼び出し側が使っている）。
  一方 `準備するもの` / `試験箇所` / `試験の概要` と、表の見出し行（`手順` / `期待結果`）は
  **元からパース対象外**。前置きは本文としてそのまま表示しているだけで、表の1行目は
  文字列に関係なく無条件スキップしている。つまり英語手順書は元々そのまま通るので、
  ここにエイリアスを足す必要はない。
  節タグ（試験対象OS等）は見出し末尾の `【Windows】` と `[Windows]` の両方を受ける。
  どちらも行末アンカーなので、`### See [docs](url)` は `)` 終わりで、`### [Draft] Login` は
  行末でないためマッチしない（`verify.mjs` に誤爆ケースを入れてある）。
- **`samples.en.js` に日本語を残さないこと**: 英語版のサンプル手順とAI整形プロンプトは
  パーサーのマーカーも含めて全部英語（`Build:` / `Attachments:` / `Glossary:` / `[Windows]` /
  `Note: `）。AI整形プロンプトは「この形式で出力せよ」とAIに指示する文面なので、ここに
  日本語キーワードが残っていると英語ユーザーに日本語での記述を強いることになる。
  `verify.mjs` はプロンプト内のコードブロックの例をそのままパーサーに通して、
  プロンプトが提示する書式が実際に解釈できることを日英とも確認している。
- **整形スキルの parser.js は同梱コピーを持たない**: `GET /TestRunner/skill.zip`
  （`test_runner/mod.rs`）はリクエストのたびに `test_runner/skill/`（SKILL.md /
  format.ja.md / format.en.md / validate.mjs）と、アプリ本体の
  `static/js/core/parser.js` を直接読んで zip を組み立てる。だから配布した zip の
  パーサーが本体から古くなることは原理的に起きない。`skill/` に `parser.js` の
  コピーを置かないこと（`verify.mjs` が `skill/parser.js` の非存在を検査する。
  `mod.rs` のテストは zip 内の `parser.js` が本体とバイト一致することを検証する）。
  ローカルの `.claude/skills/` は zip を展開して置くだけの使い捨てで、
  `/.claude` は `.gitignore` によりデプロイ先に含まれないため、正本は
  `src/api/test_runner/skill/` 側に置く（`.gitignore` はこの理由により変更しない）。
- **canonical/hreflang/sitemap の絶対URLは `api/mod.rs` の `base_url()` に集約**:
  `PUBLIC_BASE_URL`（任意。独自ドメインへ寄せたい場合に設定）があればそれを優先し、
  無ければ `X-Forwarded-Proto` + `Host` から組み立てる（Heroku等は手前でTLSを終端するため、
  schemeをヘッダから見ないとhttpになる）。SEO用のURLを足すときも個別にホスト名を書かない。
- **旧ホストからのリダイレクトは GET/HEAD のみ**: `api/legacy_host_redirect.rs` は
  `*.herokuapp.com` と `www.<正規ホスト>` へのリクエストを `PUBLIC_BASE_URL` へ 301 する
  （未設定なら無効）。POST をリダイレクトするとクライアントが GET に変えて本文を捨てるため
  API が壊れる。ショートカットから叩かれる `/recruitment/` と死活監視の `/health` は
  パスでも除外している。axum の `Redirect::permanent` は 308 なので 301 は自前で組んでいる。
- **Web UI のルートは `web_ui_router()` に集約する**: `api/mod.rs` の `run_api` に
  直接ルートを書くと、dev用の `serve_web`（bot/ExternalSourceRegistryを起こさない
  Web UI専用サーバー）とその `e2e.mjs` から見えなくなり、dev と本番でルート集合が
  ズレる。UIルート（AppStateに依存しないページ群）を増やすときは必ず `web_ui_router()`
  に足すこと。`run_api` はそれに `/recruitment/` と SwaggerUi を足すだけにする。
- **operator_combat は operator_data と意図的に別ソース**: `engine/external_source/operator_combat.rs`
  はフレームキル計算機用の戦闘生データ（元ATK/信頼度込みATK・潜在ATK・モジュールATK加算値）
  だけを持つ machine-only なソースで、`operator_data.rs`（消費素材ドメイン）には統合しない
  （オーナー方針: 消費素材ドメインと戦闘ドメインを混ぜない）。CN/JPマージ・名前解決・
  昇格オペレーターの扱いは`operator_data.rs`と同じ方針を複製している（依存を作らないため
  意図的な重複。skill_dataのblackboardフィールドも同様にフレームキル計算機専用に追加した）。
- **fk_kill_calc は3層構成でAuto/Manualの出所を持つ**: `engine/fk_kill_calc`は
  1.機械データ（operator_combatの数値+skill_dataのblackboard）→2.手動補正
  （`data/fk_kill_calc/overrides.yaml`。ブラックボードのキー名が`atk_scale`という名前で
  なかったり、1スキルが物理/術の2系統ダメージを持つ等、機械的に正しく判定できない
  実データがあるため）→3.マージ（`build_catalog`）の3層。DTOの`Valued<T>{value,source}`が
  各値の出所（`Auto`=機械判定/`Manual`=手動補正）を持ち、フロントの「補正」バッジ表示に使う。
  `overrides.yaml`の各キー(operator_id, skill_num)が実データ(fk_data/operator_combat)を
  指しているかは`cargo test`のドリフト検知テスト（`every_override_key_points_to_existing_operator_and_skill_num`）
  が保証する。ゲームデータ更新でここが落ちたら`overrides.yaml`を見直すこと。
  倍率の自動判定は「厳密一致の`atk_scale`→無ければキーが`atk_scale`/`damage_scale`で終わる
  項目のうちアルファベット順で最初のもの→どちらも無ければ1.0」の順で、候補一覧の並びは
  常に「選ばれたデフォルトが先頭、残りはキー名のアルファベット順」に正規化する
  （Seed経由/実fetch経由でblackboardの元の並びが変わっても表示順が揺れないようにするため。
  `write_seed_file`がJSONキーを再帰的にソートして書き出す影響を吸収する）。
  fk_dataシート→ゲームデータの名前解決は全角/半角括弧（`（）`↔`()`）と前後空白の表記ゆれを
  正規化してから行う（`normalize_operator_name`）。
- **fk_kill_calculator の catalog.json は web_ui_router() に入れない**: `/FrameKillCalculator`の
  ページ本体+静的ファイル（`page_router()`）は他のWeb UIツールと同じく`web_ui_router()`に
  足すが、`GET /FrameKillCalculator/catalog.json`（`catalog_router()`）だけは別軸。
  `run_api`（本番。ExternalSourceRegistryの現在値からリクエスト毎に`build_catalog`する）と
  `serve_web`（dev。起動時に`data/seed/*.json`から1回だけ組み立てたCatalogを使い回す）とで
  カタログの取得方法が全く異なり、`web_ui_router()`は状態を持たない汎用ルーターなので
  ここにAppState依存のロジックを混ぜ込めないため。`CatalogProvider`（クロージャ）を
  `run_api`/`serve_web`それぞれが個別に組み立てて`catalog_router(provider)`に渡し、
  両方が個別に`merge`する。
- **サイトアイコンは元画像から生成して commit する**: 元画像は `assets/icon/`
  （通常版 PNG と simple版 SVG）。差し替えたら
  `& "C:\Program Files\nodejs\node.exe" assets/icon/generate.mjs` を回して
  `src/api/site_icons/static/` を更新する（Playwright の Chromium で描く）。48px 以下の
  ファビコンと maskable は simple版、180px 以上は通常版を使う。simple版 SVG は星を
  `<use fill="…">` で描いており、ブラウザ以外のラスタライザ（デザインツールの PNG 書き出し等）
  はこの fill を落として星を黒く塗るので、PNG 化は必ず generate.mjs 経由にすること。
  新しいページを足したら `head_icons.html` を include する（`api/mod.rs` の
  `every_page_links_site_icons` にもパスを足す）。

## 動作確認手順

1. 環境変数を設定（PowerShell）
   ```
   $env:DISCORD_TOKEN="＜bot トークン＞"
   $env:CLOUDVISION_API_KEY="＜Google Vision API キー＞"
   $env:CHANNEL_ID_KOUKAI_KYUJIN="＜求人チャンネルID＞"
   $env:CHANNEL_ID_URANAI="＜占い館チャンネルID＞"
   $env:CHANNEL_ID_HAPPYBIRTHDAY="＜誕生日お祝いチャンネルID＞"
   $env:FK_SHEETS_API_KEY="＜FK情報スプレッドシート用 Google Sheets API キー＞"
   $env:FK_SHEETS_SPREADSHEET_ID="＜FK情報スプレッドシートID＞"
   ```
   （`.env` に一覧がある。未設定だと起動時の `ExternalSourceRegistry::load()` でpanicする）
2. `cargo run`（初回ビルドは数分）
4. web API のテスト（別ターミナル）:
   ```
   curl -X POST http://localhost:3000/recruitment/ -H "Content-Type: application/json" -d "{\"text\":\"狙撃タイプ\n工リート\n範囲攻撃\n火力\n減速\"}"
   ```
   → title と reply（responseForAI 形式）が返れば OK
5. Discord の求人チャンネルに求人画面のスクショを貼る → embed で結果表示
6. Web UI:
   - `http://localhost:3000/WLBatterySimulator`
   - `http://localhost:3000/EFRecipeCalculator`
   - `http://localhost:3000/LodChestSolver`（日本語）/ `/LodChestSolver/en`（英語）
   - `http://localhost:3000/TestRunner`（日本語）/ `/TestRunner/en`（英語）
   - `http://localhost:3000/FrameKillCalculator`（日本語専用） /
     `/FrameKillCalculator/catalog.json`
   - `http://localhost:3000/robots.txt` / `/sitemap.xml`
   - `http://localhost:3000/TestRunner/skill.zip` … 手順書整形AIエージェント用スキル
     （test-procedure-formatter）の配布zip。ダウンロードして展開すると
     `test-procedure-formatter/` フォルダができるので、そのまま `.claude/skills/`
     の下に置けば別端末でも動く（sitemapには含めていない。ページではないため）

askama はテンプレートをバイナリに埋め込むので、`templates/*.html` を直しても
**再ビルド＋再起動しないと反映されない**（`static/*` の css/js はリロードで反映）。
サーバーを起動したまま `cargo build` すると exe のロックで失敗する（os error 5）。

Web UI だけを見たい/自動テストしたいとき（bot も外部情報取得も起こさない・`.env` 不要）:

```powershell
$env:WEB_UI_PORT="3001"  # 省略時も既定3001（本番既定の3000と衝突しないので同時起動可）
cargo run --bin serve_web
```

フロントエンドの検証は2段階ある:

- `verify.mjs`（計算層。DOM非依存のモジュールを直接importして検証）
- `e2e.mjs`（表現層。Playwrightで実ブラウザを起動し、`serve_web` に対して
  実際のクリック・ホバー・アニメーションを検証する）

このマシンには **`node` が PATH に無いが実体は存在する**（`C:\Program Files\nodejs\node.exe`）。
絶対パスで呼べば普通のnodeとして動く（VS CodeのElectronをnode代わりに使う方法も
引き続き使えるが、実nodeがあるのでそちらでよい）:

```powershell
& "C:\Program Files\nodejs\node.exe" src/api/lod_chest_solver/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/test_runner/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/test_runner/e2e.mjs
& "C:\Program Files\nodejs\node.exe" src/api/fk_kill_calculator/verify.mjs
& "C:\Program Files\nodejs\node.exe" src/api/fk_kill_calculator/e2e.mjs
```

`e2e.mjs` は Playwright を使う。このマシンでは `npx playwright install` 経由で
入っているため node_modules が標準の場所（プロジェクト直下）に無く、
`%LOCALAPPDATA%\npm-cache\_npx\<hash>\node_modules\playwright` 配下にある。
`e2e.mjs` はこれを自動で探す（ハッシュ名は決め打ちしない）ので、
別マシンでセットアップし直した場合も基本はそのまま動く。

`e2e.mjs` は `cargo run --quiet --bin serve_web` を自分でspawnして`/health`を待つ
（初回ビルドがあるので最大240秒待つ）ので、事前に `serve_web` を起動しておく必要は無い。
Windows では `cargo run` の子プロセスが残ることがあるため、後始末は
`taskkill /pid <pid> /T /F` で子プロセスごと落とす。

`cargo test` と `verify.mjs`（計算層）・`e2e.mjs`（表現層のブラウザ実機）が
全部通ったうえで、必要なら `cargo run` して実際に画面を触ること。

Seedの更新（push前に思い出したら）: `cargo run --bin regen_seeds`。
`data/seed/*.json` が更新されるので `git status` で差分を確認して commit/push する。

## Python 版との出力一致の検証（推奨）

Python 版の recruitment.py / recruitFromOCR.py が手元にあるなら、同じ OCR
生テキストを Python 版と Rust 版（/recruitment/）の両方に通して、title と
reply が一致するか突き合わせると確実。特に matcher の補正辞書は Google Vision と
Apple OCR の実データでチューニングされた資産（99%以上通る）なので、ここの挙動
一致は重要。

## 元 Python 実装のメモ

- recruitFromOCR.py: OCR（Google Vision）＋タグ抽出（matchTag）。matcher.rs に移植
- recruitment.py: タグ計算（recruitDoProcess）＋整形（searchMapToStringChunks）。
  calc.rs + format.rs に移植
- RCReply: bot 応答用データクラス（embbedTitle/embbedContents/responseForAI）。
  EmbedReply / TagReply に対応
- Web API: POST /recruitment/ に OCRRawData{text, pickupOperators} → TagReplyData{title, reply}
