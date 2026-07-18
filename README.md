# astesiabot-rust

アークナイツF鯖用の便利Discord bot「アステシアちゃん」の Rust 移植版です。
[poise](https://github.com/serenity-rs/poise)/[serenity](https://github.com/serenity-rs/serenity) による Discord bot と、
[axum](https://github.com/tokio-rs/axum) による Web API を1プロセスで並行稼働させています。
元は Python 実装（[RiseiCalculatorBot-main](https://github.com/holmesfems/RiseiCalculatorBot-main)）

## Features

- 理性価値計算（`/riseimaterials` など） — ステージ周回の理性効率を比較
- オペレーター育成の消費素材計算（`/operatormastercost`, `/operatorelitecost`, `/operatormodulecost`, `/operatorcostlist`）
- 公開求人のOCR自動判定 — スクショを貼るとタグ組み合わせ結果を自動返信、Web API (`POST /recruitment/`) からも同じロジックを利用可能
- FK情報検索（`/fksearch`） — Google Sheets 連携のFKデータベースを検索
- オペレーター誕生日の自動お祝い
- スパム/連投/罠チャンネル対策などのモデレーション機能
- 武陵発電制御シミュレーター（Web UI, htmx）
- 周年ロール自動付与

## Requirement

- Rust（`cargo build` が通ることを確認済み。バージョンは `Cargo.toml` / `Cargo.lock` 参照）
- Discord Bot Token
- Google Cloud Vision API キー（公開求人のOCR用）
- Google Sheets API キー（FK情報検索用）
- OpenAI API キー（占い館機能。未実装につき現状は任意）

## アーキテクチャ

```
src/
├── lib.rs     … ライブラリクレート本体（api / bot / engine）
├── main.rs    … エントリポイント。起動時に外部情報をロードし、bot と API を並行起動
├── engine/    … bot・APIどちらにも依存しない純粋ロジック層
│   ├── external_source/       … 外部サイト情報のレジストリ（キャッシュ + Seedフォールバック）
│   ├── operator_cost_calc/ … オペレーター消費素材ドメイン
│   ├── recruit/            … 求人タグ計算ドメイン
│   └── fk_data_search/     … FK情報検索ドメイン
├── api/       … axum ルーティング（求人API、武陵発電シミュレーターWeb UI）
└── bot/       … Discord bot（コマンド・メッセージハンドラ・各種サービス）
```

詳細な設計方針・実装上の注意点は [CLAUDE.md](CLAUDE.md) を参照してください。

## セットアップ

### 1. 環境変数を設定

`.env` をプロジェクトルートに作成し、以下を設定します（`dotenvy` で読み込み。Heroku等では代わりに Config Vars を設定）。

| 変数名 | 用途 |
| --- | --- |
| `DISCORD_TOKEN` | Discord bot トークン |
| `CLOUDVISION_API_KEY` | 公開求人OCR用 Google Cloud Vision APIキー |
| `FK_SHEETS_API_KEY` / `FK_SHEETS_SPREADSHEET_ID` | FK情報検索用 Google Sheets API |
| `CHANNEL_ID_KOUKAI_KYUJIN` | 公開求人チャンネルID |
| `CHANNEL_ID_HAPPYBIRTHDAY` | 誕生日お祝いチャンネルID |
| `CHANNEL_ID_URANAI` | 占い館チャンネルID |
| `AUTODEL_1`〜`AUTODEL_6` | 罠チャンネルID（自動削除・自動BAN対象） |
| `REPORT_CHANNEL_ID` | 通報先チャンネルID |
| `ANNIROLEID` | 周年ロールID |
| `OPENAI_API_KEY` / `OPENAI_MODEL` | 占い館（OpenAI Responses API） |
| `GUILD_ID_F` / `ROLE_ID_YOUTUBE_MEMBER` / `ROLE_ID_SERVER_BOOSTER` | 占い館の課金ロール判定 |
| `PORT` | Web API の待受ポート（未設定時は既定値。Herokuでは自動設定される） |

未設定の環境変数がある場合、該当機能の初期化時にpanicすることがあります。

### 2. 起動

```powershell
cargo run
```

初回ビルドには数分かかります。

### 3. 動作確認

Web API:
```powershell
curl -X POST http://localhost:3000/recruitment/ -H "Content-Type: application/json" -d "{\"text\":\"狙撃タイプ\n工リート\n範囲攻撃\n火力\n減速\"}"
```
`title` と `reply` が返れば正常です。

Discord bot: 求人チャンネルに求人画面のスクショを貼ると embed で結果が返ります。

### Seedデータの更新

外部情報（オペレーターデータ・スキルデータ等）の起動時fetchに失敗した場合のフォールバックとして `data/seed/*.json` を保持しています。更新する場合は以下を実行し、差分をコミットしてください。

```powershell
cargo run --bin regen_seeds
```

## デプロイ (Heroku)

`Procfile` を同梱しています。Rust用ビルドパック（例: [emk/heroku-buildpack-rust](https://github.com/emk/heroku-buildpack-rust)）を設定し、上記の環境変数を Config Vars として登録すればデプロイ可能です。

## License

[MIT License](LICENSE)

## Author

[holmesfems](https://github.com/holmesfems)
