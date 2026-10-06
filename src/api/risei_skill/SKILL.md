---
name: arknights-risei-api
description: アークナイツ（明日方舟 / Arknights）の理性効率・理性価値を astesiabot の REST API（https://astesiabot.com/api/risei）から取得する。Discord bot の /riseimaterials /riseistages /riseievents /riseilists /riseikakin（全体比較・グローバル版）と同じ計算結果をJSONで返す。「理性効率」「理性価値」「◯◯はどこで掘る」「◯◯の周回ステージ」「1-7の効率」「イベントステージの効率」「資格証効率」「特別引換証」「契約賞金(CC)」「基準マップ」「課金パック効率」「パックはお得か」のように、素材の集め方やステージ・交換所の効率の話が出たら、理性効率動画の台本づくり・ファクトチェック中であっても必ずこのスキルを使うこと。記憶やWiki知識で効率の数値を答えず、必ずAPIを叩くこと。
---

# astesiabot 理性価値計算 API

ベースURL: `https://astesiabot.com/api/risei`（認証なし・全部 GET・応答は JSON）

値はサーバー内のキャッシュで、penguin-stats のドロップデータから **120分ごと** に再計算される。
応答の `updated_at`（UTC）が計算時刻。動画で数値を引用するときはこの日時も控えておくこと。

## まず使い方一覧を読む

```bash
curl -s https://astesiabot.com/api/risei
```

エンドポイント・引数・素材カテゴリ一覧（`material_categories`）・効率表の種類（`list_kinds`）・
両サーバの最終更新時刻が返る。カテゴリ名などが分からなければ推測せずここを見る。

## エンドポイント

| パス | 用途 | 必須引数 |
|---|---|---|
| `/api/risei/materials` | 素材カテゴリ別の周回ステージ（総合効率の降順） | `target`（日本語名 例: `砥石`、またはキー=中国語名 例: `研磨石`） |
| `/api/risei/stages` | 恒常ステージ（メイン・恒常サイスト）の効率 | `stage`（前方一致・大文字小文字区別 例: `1-7`, `12-`, `R8-`） |
| `/api/risei/events` | 期間限定イベントステージ（過去・未開催含む） | `stage`（前方一致 例: `SV-8`, `IW-`） |
| `/api/risei/lists/{kind}` | 各種効率表 | なし。`kind` = `base_maps` / `values` / `te2` / `te3` / `special` / `cc` / `kakin` |

共通の任意引数:

- `server=global`（既定。グローバル版＝日本版基準）/ `server=mainland`（大陸版基準。新ステージ・新素材込み）
- `limit=N`（materials/stages/events のステージ数上限。省略時は全件）

日本語は URL エンコードする（`curl -G --data-urlencode` が楽）:

```bash
curl -sG https://astesiabot.com/api/risei/materials --data-urlencode "target=砥石" -d limit=5
curl -sG https://astesiabot.com/api/risei/stages -d stage=1-7
curl -sG https://astesiabot.com/api/risei/events -d stage=SV-8 -d server=mainland
curl -s  https://astesiabot.com/api/risei/lists/te2
```

**Windows（Git Bash / PowerShell）の curl は日本語の引数を UTF-8 で送らない**（システムのコードページで
化けて「不明な素材カテゴリです: �u��」のような404になる）。Windows では UTF-8 で
パーセントエンコードしたものを URL に直接書くこと:

```bash
q=$(python -c 'import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))' 砥石)
curl -s "https://astesiabot.com/api/risei/materials?target=$q&limit=5"
```

日本語名の代わりにキー（中国語名）を使っても同じ問題が起きるので、エンコードは必須。

## 値の読み方

- 効率・ドロップ率などの比率は **1.0 = 100%**（例: `0.857` → 85.7%）。表示時は%に直す
- `total_efficiency` / `efficiency`: 総合理性効率（ドロップ品の理性価値合計 ÷ 消費理性）
- `main_item_efficiency`: そのカテゴリの素材だけで見た効率。`promotion_efficiency`: 昇進素材だけで見た効率
- `confidence_3sigma`: 効率の99%信頼区間（3σ）の幅。基準マップは0
- `time_cost`: 倍速でのクリア時間（秒）。`drop_per_minute`: 倍速1分あたりの入手数（中級素材換算）。
  クリア時間データが無いステージは `null`
- `max_times`: ドロップ統計の試行数。少ないステージは誤差が大きいので注意して扱う
- `sanity_cost`: 消費理性
- 効率表（lists）の `std_dev` は1σ。Discord 版の「±」表示は 2σ（`std_dev × 2`）
- `effective_server`: 実際に計算に使ったサーバ。グローバル版に無いステージや大陸版先行カテゴリ
  （`group: "new"`）を指定すると自動で `mainland` になる。動画で「日本版の数値」と言うときは
  ここが `global` であることを確認する
- ステージ名の `(Re)` 等は同名ステージの別開催（復刻）

### 課金パック効率（`/api/risei/lists/kakin`）

Discord の /riseikakin「全体比較(グローバル)」と同じ値。**グローバル版のみ**（`server=mainland` は400）。
他の効率表と違い `items` ではなく `baselines` と `packs` を返す。

- `packs`: 現在 bot に登録中の期間限定パック。総合効率の降順
  - `total_efficiency`: 総合効率。10000円恒常パック（=1.0）と比べて理性価値が何倍お得か
  - `gacha_efficiency`: ガチャ効率。ガチャ数（合成玉・純正源石・スカウト券の換算）だけで見た同様の倍率
  - `price_jpy`: 値段（円）。`value_jpy`: マネー換算（10000円恒常パックと同じレートで買ったら何円か）
  - `total_value`: 合計理性価値。`total_originium`: 純正源石換算。`gacha_count`: ガチャ数
  - `contents`: 中身 `{name, count, value_jpy}`。`value_jpy` の合計はパックの `value_jpy` と一致する
- `baselines`: 比較用の恒常パック（Discord 版の「参考用課金効率」。10000円恒常/初回・月間スカウト・月パス・初心者向け等）
- パックの登録・値段は手動メンテ。販売終了したパックは載らないが、反映が遅れることはある

## 間違えたとき

エラーは常に JSON で `{error, did_you_mean?, usage?, hint?}` を返す。読んで直して再試行すればよい。

- ステージ名が無い → `did_you_mean` に近いステージ名。恒常/イベントを取り違えていれば
  `hint` に正しいエンドポイントが入る
- 素材カテゴリが無い → `available` に全カテゴリ
- 未知の引数（例: Discord 版の `is_global`）や `server` の値違い → 400 と `usage`
- 存在しないパス → `did_you_mean` に近いエンドポイント

## 注意

- claude.ai から使う場合、コード実行のネットワーク許可に `astesiabot.com` を含める必要がある
  （許可されていないと接続できない。そのときはユーザーに設定を頼む）
