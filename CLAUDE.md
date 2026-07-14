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
│   │   │                機能側がそこから参照する（例: birthday.rs が operator_names を参照）
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
│   │   └── operator_names.rs … オペレーターCN→JA名変換。Arknights CN/JP character_table.json
│   │                            をfetch。SEED_PATH = data/seed/operator_names.json
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
    ├── commands/  … スラッシュコマンド（ping/echo/add）1コマンド1ファイル
    └── services/
        ├── moderation.rs  … スパム検知・連投/爆撃対応・罠チャンネル自動削除・全体通知BAN（実装済み）
        ├── anniversary.rs … 周年ロール付与など
        ├── birthday.rs    … オペレーター誕生日自動お祝い（毎日 JST 0:00、operator_names を利用）
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
└── seed/                      … outer_source の Seed（`cargo run --bin regen_seeds` で
                                   生成し、git commitして含めておく。詳細は下記ポイント参照）
    └── operator_names.json
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

## 元 Python 実装のメモ

- recruitFromOCR.py: OCR（Google Vision）＋タグ抽出（matchTag）。matcher.rs に移植
- recruitment.py: タグ計算（recruitDoProcess）＋整形（searchMapToStringChunks）。
  calc.rs + format.rs に移植
- RCReply: bot 応答用データクラス（embbedTitle/embbedContents/responseForAI）。
  EmbedReply / TagReply に対応
- Web API: POST /recruitment/ に OCRRawData{text, pickupOperators} → TagReplyData{title, reply}
