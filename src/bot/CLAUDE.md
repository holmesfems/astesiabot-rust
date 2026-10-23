# bot（Discord bot。poise / serenity）

## 構成

```
mod.rs       … run_bot。setup() で ChannelRouting::from_env()・誕生日チャンネルの解決（未設定なら panic）・
               誕生日スケジューラ・（環境変数が揃っていれば）YouTube お知らせを spawn
data.rs      … Data { state, channel_routing }
handler.rs   … メッセージの振り分け（自分 → bot/webhook → スパム → チャンネル別）。
               ChannelRouting（振り分け先チャンネルの定義と env 解決）も同居。
               振り分け先を増やすときはここだけ触る（data.rs / mod.rs は変えない）
reply.rs     … EmbedReply（embed の分割送信。reply_marker を footer に出す）
utils/       … env.rs（channel_id_env 等。不正値なら起動時 panic）/ xlsx.rs（riseiXXX の csv_file 添付）
commands/    … スラッシュコマンド（1コマンド1ファイル）。fksearch.rs / risei/ / operator_cost_calc/
services/
  moderation.rs     … スパム検知・連投/爆撃・罠チャンネル・全体通知 BAN
  recruit/          … 公開求人チャンネル（OCR → 計算 → embed、リプライでのタグ編集）
  uranai/           … 占い館（OpenAI のチャット。functioncalling/ に bot 機能を呼ぶ関数）
  reply_dispatch.rs … コマンド応答 embed へのリプライを、footer のマーカーから由来コマンドに戻す
  birthday.rs       … 毎日 JST 0:00 にオペレーターの誕生日をお祝い
  anniversary.rs    … 周年ロール付与など
  youtube_notify.rs … YouTube 新着・配信のお知らせ
```

## 守ること

- **bot / webhook の発言はモデレーションに渡さない**: 他 bot の返信はメンバーがコマンドで好きに
  出せるので、author で連投検知すると他 bot を BAN させられる。代わりにスラッシュコマンドへの返信は
  `moderation::handle_command_reply` で interaction_metadata の「打った人」（`Actor`）の操作として数える。
  連投/爆撃検知のみ（本人の発言と同じ履歴・閾値）、1コマンド＝1回（フォローアップは数えない）、
  ボタン/モーダル由来は数えない。遡及削除はその人が打ったコマンドへの bot の返信を消す。
- **占い館の function calling**: `data/uranai/toolList.yaml` は `build_tool_list_entries()` と一致して
  いる必要がある（`tool_list_yaml_matches_registered_functions`）。関数を足したり description /
  parameters を変えたら `cargo run --bin regen_uranai_tools` で作り直して commit する。
- **fksearch のオートコンプリート**は1時間 TTL のチェックを通さず fk_data を直接読む（コマンド本体は TTL 付き）。

## YouTube のお知らせ（youtube_notify.rs）

- 10分ごとに YouTube Data API v3 の `playlistItems`（チャンネルID の `UC` を `UU` にしたアップロード一覧。
  `YOUTUBE_API_KEY`）を読む。**RSS（`/feeds/videos.xml`）に戻さない**: Heroku（AWS）の IP からは
  どのチャンネルでも404（2026-10 に `heroku run` で確認。動画ページ・チャンネルページは読める）。
- API キーは URL に載るので、reqwest のエラーは `without_url()` してからログに出す。
- ふつうの動画: 公開から24時間以内・ショート以外（`/shorts/<ID>` が200ならショート、303なら動画）・
  未投稿のものを、アステシアの宣伝文（`UranaiState::write_in_character`）付きで投稿。生成に失敗したら決まった文面。
  宣伝文は AI の出力なので allowed_mentions で `ROLE_ID_YOUTUBE_NOTIFY` だけに絞る。
- 配信: 動画ページの `ytInitialPlayerResponse`（`isLiveContent` / `isUpcoming` / `isLiveNow`）で種類を決め、
  配信中になったら `CHANNEL_ID_STREAM_NOTIFY`（任意。無ければ配信は流さない）に決まった文面で投稿し
  `ROLE_ID_STREAM_NOTIFY`（任意）をメンション。予定・プレミア前は始まるまで待ち、過去の配信は流さない。
  配信は枠を立てた時刻より後に始まるので、24時間の絞り込みはふつうの動画だけに掛ける。
- **二重投稿の防止はファイルに持たない**: お知らせチャンネルの直近50件に bot がその動画 ID を
  貼ったかで判定する（実行時のファイル書き込みは再起動で消えるため）。種類が決まった動画は
  起動中だけメモリに覚えて、動画ページを読み直さない。
- `YOUTUBE_CHANNEL_ID`・`YOUTUBE_API_KEY`・`CHANNEL_ID_YOUTUBE_NOTIFY` のどれかが無ければ起動しない
  （他の機能と違い panic しない。環境変数より先にデプロイしても動くように）。
- 本物の YouTube での確認: `cargo test --lib -- --ignored youtube_live`
