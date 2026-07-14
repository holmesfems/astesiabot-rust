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
├── main.rs        … bot 本体のエントリポイント。RecruitEngine / OuterSourceRegistry を
│                    起動時ロード → bot と api に Arc で共有。1日1回
│                    OuterSourceRegistry::refresh_all() を叩くループも起動
├── bin/
│   └── regen_seeds.rs … outer_source の Seed（data/seed/*.json）を手動再生成する
│                          独立ツール。main.rs には依存しない。使い方は後述
├── engine/
│   ├── outer_source/ … 外部サイトから取得する情報のレジストリ（bot にも api にも
│   │   │                依存しない）。起動時に一括fetchしてメモリ保持し、以後は
│   │   │                機能側がそこから参照する（例: birthday.rs が operator_data を参照）
│   │   ├── mod.rs        … OuterSourceRegistry。load / refresh_all（定期実行用の一括fetch）/
│   │   │                    refresh_by_name（機能側からの個別オンデマンド更新用）。
│   │   │                    SEED_JOBS（regen_seeds が使うSeed生成ジョブ一覧）もここ。
│   │   │                    情報源を増やす手順はこのファイルのコメントを参照
│   │   ├── cache.rs      … Source<T>。fetch結果をメモリ保持しつつ、任意でSeed（fetch失敗時の
│   │   │                    代替用JSONファイル）を読む。起動時fetch失敗→Seedがあれば使用、
│   │   │                    無ければpanic。起動後の再fetch失敗→直前のメモリを保持したまま継続。
│   │   │                    Seedの書き込み（write_seed_file）は実行時には呼ばない
│   │   │                    （regen_seeds からのみ使う。理由は下記ポイント参照）
│   │   ├── http.rs       … 全情報源共通のfetch戦略（7sタイムアウト・最大10回リトライ）
│   │   ├── operator_data.rs … オペレーターCN→JA名変換 +
│   │   │                       昇進/スキル特化/モジュール消費素材の生データ。character_table.json /
│   │   │                       uniequip_table.json / char_patch_table.json をまとめて1回のfetchで
│   │   │                       構築する（旧operator_names.rsはこれに統合済み）。
│   │   │                       SEED_PATH = data/seed/operator_data.json
│   │   └── skill_data/      … スキルID→表示名+説明文（skill_table.json をfetch）。
│   │       ├── mod.rs          … SkillData（旧skill_names.rsのSkillNamesを統合）。get_str/get_description。
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
│   └── recruit/   … ★求人ドメインの純粋ロジック（bot にも api にも依存しない）
│       ├── mod.rs     … RecruitEngine。process_from_ocr（API用）/ process_for_embed（bot用）
│       ├── model.rs   … Operator, Tag, TagType
│       ├── calc.rs    … タグ計算エンジン（ピックアップ対応、future オペレーター実装済み）
│       ├── matcher.rs … OCR生テキスト → タグ抽出（fancy-regex、3言語辞書＋誤字補正）
│       └── format.rs  … 出力整形（display_chunks / response_for_ai / make_title / 分割）
├── api/
│   ├── mod.rs             … axum。AppState、run_api
│   ├── recruitment.rs     … POST /recruitment/ （Python の doRecruitment と完全一致）
│   └── wl_battery_simulator/ … 武陵発電制御シミュレーター（askama + htmx の Web UI）
│       ├── mod.rs        … ルーター（index/calculate/static配信）
│       ├── battery_sim.rs… シミュレーションエンジン（Python版 batterySim.py 移植）
│       ├── optimizer.rs  … 図面ごとの最適化（Python版 optimizer.py 移植。最大発電量超は按分計算）
│       ├── templates/    … index/result/chart/error.html
│       └── static/       … css/画像/tutorial html
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
├── recruitmentOperators.json  … オペレーターDB（main 153体 + future プール）
├── tagList.json               … タグ種別定義
├── tagJaToJa.yaml / tagEnToJa.yaml / tagZhToJa.yaml … 3言語辞書
├── birthdayRev.yaml           … 日付→誕生日オペレーター(中国語名)一覧
├── customZhToJa.yaml          … CN限定オペレーターのCN→JA名前フォールバック
├── golden/operator_cost_calc/ … Python版charmaterials.pyの出力をゴールデンJSON化したもの。
│                                 `ref_python/.../dump_charmaterials_golden.py`で生成し、
│                                 bot/commands/operator_cost_calc の golden_tests が実ネットワーク
│                                 テスト(#[ignore])で突き合わせる。詳細は下記ポイント参照
└── seed/                      … outer_source の Seed（`cargo run --bin regen_seeds` で
                                   生成し、git commitして含めておく。詳細は下記ポイント参照）
    ├── operator_data.json
    ├── operator_names.json    … 旧operator_names.rsが残したSeed。operator_data統合後は
    │                             regen_seedsでは更新しない。名前解決が壊れていないかの
    │                             ゴールデン参照として意図的に残置している
    └── skill_data.json
```

依存方向: recruit / outer_source は何にも依存しない純粋ロジック。bot と api が
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
- **outer_source の fetch 失敗ポリシー**: 起動時fetch失敗 → Seedがあれば使用、
  無ければ panic。起動後の再fetch失敗 → 直前のメモリを保持して継続（panicしない）。
  この非対称性（起動時はpanicし得る／再fetchはしない）を崩さないこと。
- **Seedは実行時に書き込まない**: Heroku 等は実行時のファイル書き込みが dyno
  再起動・再デプロイで揮発するため、`Source` は起動時に Seed を**読む**だけで、
  fetch成功時に書き戻すことはしない。Seedの更新は `cargo run --bin regen_seeds`
  （main.rs 非依存の独立ツール。`engine/outer_source/mod.rs` の `SEED_JOBS` を
  順に実行して `data/seed/*.json` を書き換える）を手元で実行し、差分を
  `git commit`/`push` してリポジトリに含める運用。push前に思い出したタイミングで
  都度実行すればよい（自動化はしていない）。
- **fetchの共通戦略**: `engine/outer_source/http.rs` の `client()` /
  `fetch_json_with_retry()`（7sタイムアウト・最大10回リトライ）が全情報源共通。
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
- **skill_data の説明文組み立てはPython版と意図的に異なる**: `engine/outer_source/skill_data/description.rs`
  はPython `SkillIdToName.SkillItem`のblackboard置換を再現するが、Python版の`cleanStr`副作用
  （数値フォーマット指定子の小数点を巻き込んで壊す/`chain.max_target`を`max_target`へ誤統合する）
  は踏襲せず実データに即して正しく表示する。理由の詳細は`description.rs`冒頭コメント参照。
- **operator_cost_calc は説明文をembedに表示していない**: `SkillData::get_description`は用意済みだが、
  skillMasterCostのembedへの表示はまだ配線していない（スコープ外）。表示したくなったら
  `bot/commands/operator_cost_calc/operatormastercost.rs`から呼べばよい。
- **operator_cost_calc のゴールデンテストは理性価値に許容誤差を持つ**: risei_calculator_engine
  の基準マップ選定は乱数を使うため（近接タイの複数カテゴリが実行毎に異なる基準ステージへ
  収束し得る。Python版も`random.choice`で同様）、理性価値はPython版と実行毎に僅かに
  （観測上0.02未満）ズレる。`bot/commands/operator_cost_calc/mod.rs`の`golden_tests`は
  数値のみ許容誤差付きで比較し、近接タイによる隣接2件の順序入れ替えも許容する
  （`lines_match_with_adjacent_swap_tolerance`）。ゲームデータ更新でランキング内容自体が
  変わった場合は`ref_python/RiseiCalculatorBot-main/dump_charmaterials_golden.py`を再実行して
  `data/golden/operator_cost_calc/*.json`を更新すること（`regen_seeds`もセットで実行）。

## 動作確認手順

1. 環境変数を設定（PowerShell）
   ```
   $env:DISCORD_TOKEN="＜bot トークン＞"
   $env:CLOUDVISION_API_KEY="＜Google Vision API キー＞"
   $env:CHANNEL_ID_KOUKAI_KYUJIN="＜求人チャンネルID＞"
   $env:CHANNEL_ID_URANAI="＜占い館チャンネルID＞"
   $env:CHANNEL_ID_HAPPYBIRTHDAY="＜誕生日お祝いチャンネルID＞"
   ```
   （`.env` に一覧がある。未設定だと bot 起動直後の setup() で panic する）
2. `cargo run`（初回ビルドは数分）
4. web API のテスト（別ターミナル）:
   ```
   curl -X POST http://localhost:3000/recruitment/ -H "Content-Type: application/json" -d "{\"text\":\"狙撃タイプ\n工リート\n範囲攻撃\n火力\n減速\"}"
   ```
   → title と reply（responseForAI 形式）が返れば OK
5. Discord の求人チャンネルに求人画面のスクショを貼る → embed で結果表示

Seedの更新（push前に思い出したら）: `cargo run --bin regen_seeds`。
`data/seed/*.json` が更新されるので `git status` で差分を確認して commit/push する。

## Python 版との出力一致の検証（推奨）

Python 版の recruitment.py / recruitFromOCR.py が手元にあるなら、同じ OCR
生テキストを Python 版と Rust 版（/recruitment/）の両方に通して、title と
reply が一致するか突き合わせると確実。特に matcher の補正辞書は Google Vision と
Apple OCR の実データでチューニングされた資産（99%以上通る）なので、ここの挙動
一致は重要。

## 未実装 / TODO（今後の作業）

- [x] cargo build を通す
- [x] moderation.rs（旧 spam.rs）: スパム検知ロジック（連投・全体通知・罠チャンネル）。
      連投カウントは ModerationState に Mutex<HashMap<...>> で保持
- [ ] uranai.rs: 占い館。ChatGPT/OpenAI API 連携でメッセージに応答生成
- [x] future オペレーター対応（calc.rs、RECRUIT_FUTURE_HOUR で実装時刻判定）
- [ ] embed をフィールド分割にするか要検討（現状は description 結合＋1900字分割）
- [x] charmaterials.py 移植（operator_cost_calc + operator_cost_calc コマンド4種:
      operatormastercost/operatorelitecost/operatormodulecost/operatorcostlist）
- [x] SkillIdToName（スキル説明文のblackboard置換）の移植。`engine/outer_source/skill_data/`
      （データ層まで。skillMasterCostのembedへの表示配線は未着手）

## 元 Python 実装のメモ

- recruitFromOCR.py: OCR（Google Vision）＋タグ抽出（matchTag）。matcher.rs に移植
- recruitment.py: タグ計算（recruitDoProcess）＋整形（searchMapToStringChunks）。
  calc.rs + format.rs に移植
- RCReply: bot 応答用データクラス（embbedTitle/embbedContents/responseForAI）。
  EmbedReply / TagReply に対応
- Web API: POST /recruitment/ に OCRRawData{text, pickupOperators} → TagReplyData{title, reply}
