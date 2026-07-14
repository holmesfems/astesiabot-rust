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

## アーキテクチャ

```
src/
├── main.rs        … RecruitEngine を起動時ロード → bot と api に Arc で共有
├── engine/
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
    ├── mod.rs     … run_bot(token, state)
    ├── data.rs    … Data { state: Arc<AppState> }
    ├── handler.rs … 汎用メッセージハンドラ（自分→スパム→bot→チャンネル別 の順で振り分け）
    ├── channels.rs… チャンネルID定数（★要設定、下記）
    ├── commands/  … スラッシュコマンド（ping/echo/add）1コマンド1ファイル
    └── services/
        ├── moderation.rs  … スパム検知・連投/爆撃対応・罠チャンネル自動削除・全体通知BAN（実装済み）
        ├── anniversary.rs … 周年ロール付与など
        ├── uranai.rs      … 占い館（骨組みのみ、未実装。ChatGPT API連携が必要）
        └── koukai_kyujin/ … 公開求人
            ├── mod.rs … OCR → process_for_embed → embed 送信
            └── ocr.rs … Google Vision REST（v1〜v1p4beta1 を切替。連続失敗対策）

data/  … 実行時に読み込む（カレントディレクトリ基準なのでプロジェクトルートで実行）
├── recruitmentOperators.json  … オペレーターDB（main 153体 + future プール）
├── tagList.json               … タグ種別定義
├── tagJaToJa.yaml / tagEnToJa.yaml / tagZhToJa.yaml … 3言語辞書
```

依存方向: recruit は何にも依存しない純粋ロジック。bot と api が recruit に依存。
これにより求人計算ロジックを bot でも web API でも共有できる。

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

## 動作確認手順

1. 環境変数を設定（PowerShell）
   ```
   $env:DISCORD_TOKEN="＜bot トークン＞"
   $env:CLOUDVISION_API_KEY="＜Google Vision API キー＞"
   ```
2. `src/bot/channels.rs` の KOUKAI_KYUJIN を実際の求人チャンネルID に置き換え
3. `cargo run`（初回ビルドは数分）
4. web API のテスト（別ターミナル）:
   ```
   curl -X POST http://localhost:3000/recruitment/ -H "Content-Type: application/json" -d "{\"text\":\"狙撃タイプ\n工リート\n範囲攻撃\n火力\n減速\"}"
   ```
   → title と reply（responseForAI 形式）が返れば OK
5. Discord の求人チャンネルに求人画面のスクショを貼る → embed で結果表示

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
