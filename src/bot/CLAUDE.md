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
  moderation/       … スパム検知・連投/爆撃・罠チャンネル・全体通知 BAN（mod.rs が入口と状態、detect.rs が検知、action.rs が処断と通報）
  recruit/          … 公開求人チャンネル（OCR → 計算 → embed、リプライでのタグ編集）。local_ocr.rs がローカルのタグ枠 OCR の bot 側
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
  ボタン/モーダル由来は数えない。数える時刻は返信の到着ではなく interaction の作成時刻（ID から取れる）で、
  検知の窓より古い返信は数えない（interaction token は15分間フォローアップできるので、溜めて一斉に出されると
  打った人が BAN される）。遡及削除はその人が打ったコマンドへの bot の返信を消す。
- **占い館の function calling**: `data/uranai/toolList.yaml` は `build_tool_list_entries()` と一致して
  いる必要がある（`tool_list_yaml_matches_registered_functions`）。関数を足したり description /
  parameters を変えたら `cargo run --bin regen_uranai_tools` で作り直して commit する。
- **占い館の会話**（`uranai/conversation.rs` が純粋ロジック、`mod.rs` がワーカー）: 考え中に来た発言は捨てずに貯め、
  返事を送ったあとにまとめて1回で AI に渡す（最大10件・4000字・添付10件/50MB。超えたら古い方から落とす）。
  会話は `std::sync::Mutex` で持ち `.await` をまたいで握らない。ワーカーが panic しても `WorkerGuard` が busy を下ろす。
  reset は世代を進め、考え中だった返事は送るが新しい履歴には積まない。AI の呼び出しは1日（JST）`CHAT_DAILY_CALL_LIMIT`
  回まで（任意。既定200）。ツールのログに引数（ユーザー由来）を出さない。ツールの往復の2回目以降も `instructions` を送る
  （`previous_response_id` では引き継がれない）。

## メンション会話（uranai/mention/）

鯖（GUILD_ID_F）のどのチャンネルでも、アステシアにメンションすると会話が始まる。占い館の会話エンジン・OpenAI クライアント・
キャラ設定・日次上限を共有し、状態遷移は `mention/state.rs`（純粋ロジック。ID は素の u64）、Discord との出入りは `mention/mod.rs`。

- 呼び出しは本文の `<@bot>` / `<@!bot>`（`msg.mentions` と AND）。**リプライの自動メンションだけでは始めない**。
  handler では reply_dispatch の後ろ。占い館・公開求人の中のスレッドと `CHANNEL_IDS_MENTION_CHAT_DENY`
  （任意・カンマ区切り。スレッドは親、ふつうのチャンネルはカテゴリでも照合）では受けない。
- 課金ロールが無い人は決まった文面の一言＋10分無視。案内はチャンネル単位でも30秒は連投しない。
- 会話中は「参加者」（始めた人＋メンションして加わった課金ロール持ち、最大5人）の発言だけをメンション無しで拾う。
  参加者以外の発言は AI に渡さない。
- 返事の要否は AI が構造化出力（`{action: reply|silent|end, message}`）で決める。最後のラウンドの message だけを読み、
  読めなければエラー扱い（生の JSON を出さない）。画像を作ったのに silent なら送る。
- 閉じる: end（AI のお別れ）・呼び出し20回のあとの発言（決まった文面）・silent 5回連続（黙って）・エラー3回連続・
  参加者の最後の発言から10分（黙って。タイマーは持たず次の発言で掃除）・送信の403/404・`@アステシア reset`（参加者か管理者）。
  同時に開ける会話は鯖全体で3つ。画像は1会話1枚。
- 本物の OpenAI で構造化出力とツール一式が通るかの確認: `cargo test --lib -- --ignored structured_output_live --nocapture`
  （.env の OPENAI_API_KEY を使う）。
- **fksearch のオートコンプリート**は1時間 TTL のチェックを通さず fk_data を直接読む（コマンド本体は TTL 付き）。

## YouTube のお知らせ（youtube_notify.rs）

- 10分ごとに YouTube Data API v3 の `playlistItems`（チャンネルID の `UC` を `UU` にしたアップロード一覧。
  `YOUTUBE_API_KEY`）を読む。**RSS（`/feeds/videos.xml`）に戻さない**: Heroku（AWS）の IP からは
  どのチャンネルでも404（2026-10 に `heroku run` で確認。動画ページ・チャンネルページは読める）。
- API キーは URL に載るので、reqwest のエラーは `without_url()` してからログに出す。
- ふつうの動画: 公開から24時間以内・ショート以外（`/shorts/<ID>` が200ならショート、303なら動画）・
  未投稿のものを、アステシアの宣伝文（`UranaiState::write_in_character`）付きで投稿。生成に失敗したら決まった文面。
  宣伝文は AI の出力なので allowed_mentions で `ROLE_ID_YOUTUBE_NOTIFY` だけに絞る。
- 配信: Data API の `videos`（`snippet.liveBroadcastContent` が live / upcoming、`liveStreamingDetails` の有無）で
  種類を決める。**動画ページの `ytInitialPlayerResponse` だけで決めない**: Heroku から読むと `isLiveContent` が
  欠けることがあり、配信中の枠を動画としてお知らせした（2026-10-26）。ページは「終わった配信」と「公開済みの
  プレミア」の見分けにだけ使い、`isLiveContent` が無ければ配信とみなす。
  配信中になったら `CHANNEL_ID_STREAM_NOTIFY`（任意。無ければ配信は流さない）に決まった文面で投稿し
  `ROLE_ID_STREAM_NOTIFY`（任意）をメンション。予定・プレミア前は始まるまで待ち、過去の配信は流さない。
  配信は枠を立てた時刻より後に始まるので、24時間の絞り込みはふつうの動画だけに掛ける。
- **二重投稿の防止はファイルに持たない**: お知らせチャンネルの直近50件に bot がその動画 ID を
  貼ったかで判定する（実行時のファイル書き込みは再起動で消えるため）。種類が決まった動画は
  起動中だけメモリに覚えて、動画ページを読み直さない。
- `YOUTUBE_CHANNEL_ID`・`YOUTUBE_API_KEY`・`CHANNEL_ID_YOUTUBE_NOTIFY` のどれかが無ければ起動しない
  （他の機能と違い panic しない。環境変数より先にデプロイしても動くように）。
- 本物の YouTube での確認: `cargo test --lib -- --ignored youtube_live`

## 公開求人のローカル OCR（recruit/local_ocr.rs）

画像をまずローカルのタグ枠 OCR（`engine::recruit::tag_ocr`）で読み、読み切れなければ Google Vision に回す。
テンプレート（`data/recruitment/tag_templates/`）が無い・壊れていれば起動時に panic する。

- `LOCAL_OCR_MODE`: `off`（未設定・不明な値。不明ならログに警告）/ `shadow` / `on`。起動時に一度だけ読む。
  - `shadow`: ローカルも読むが返信は**常に Google の結果**。ローカルが読めたとき、Google のテキストから取れたタグ集合と
    比べて agree / disagree / Google がタグ無し / ローカル不確か（理由別）を数える。
  - `on`: ローカルが読み切れた画像は Google を呼ばない。読めなければ Google（NoText・Unavailable の扱いは従来どおり）。
  - 導入の順番: `shadow` で本番に出す → DM とログで不一致が無い（または説明がつく）ことを確かめる → `on`。
    戻すときは `off`（環境変数を変えて再起動するだけ）。
- `LOCAL_OCR_REPORT_USER_ID`（任意。Discord のユーザー ID）: 設定すると、shadow で不一致が出たときにその人へ DM する
  （local と Google のタグ・枠ごとの 1 位 / 2 位と類似度・元メッセージへのリンク。画像・投稿者の情報は入れない）。
  あわせて毎日 JST 0:00 に shadow の集計を 1 行で DM する（前回から 1 枚も処理していなければ送らない。集計は送るたびに
  0 に戻り、再起動でも消える）。不正な値なら起動時に panic、未設定ならログだけ。DM の失敗は警告のログだけで返信には影響しない。
  Heroku のログは約 1500 行しか残らないので、shadow の結果はログだけに頼らない。
- 日本語版の読めない画像の監視（shadow・on 共通。ゲームの画面変更の検知）: ローカルが OCR の理由で読めず、Google のテキストが
  日本語で 5 タグ取れた画像を「日本語版の読めなかった」と数える（ダウンロード・デコード等の失敗や Google の NoText は数えない）。
  連続 3 枚で `LOCAL_OCR_REPORT_USER_ID` に DM（ローカルが 1 枚読めるまで再送しない）。同じ画像は直近 20 枚ぶんのハッシュで
  1 回だけ数える。日次まとめに「日本語版 読めた n/m」が出る（shadow・on 両方）。状態はメモリだけで再起動で消える。
- ログは全部 `[local_ocr]` で始める（1 枚ごとに、どちらの経路を取ったかと 5 枠の最小スコア）。
- 添付はローカルでは `attachment.url` から自分でダウンロードする（10 秒・10MB まで。超えれば Google へ）。
  デコードは `spawn_blocking` の中で、幅・高さ 8000px・2000 万画素・メモリ 256MB の上限つき（悪意ある画像でメモリを
  溢れさせない）。同時に読むのは 2 枚まで。ローカルの失敗はすべて「読めなかった」として Google に回す。
- DM などの文面は `local_ocr.rs` の純粋関数（`disagreement_message` / `summary_message`。2000 文字で切る）が作り、
  ここの単体テストで確かめる。
- 本物の Discord で DM が届くかの確認: `$env:LOCAL_OCR_REPORT_USER_ID = "<ID>"; cargo test --lib -- --ignored local_ocr_dm_live`
  （.env の bot から送る。送り先がその bot と同じ鯖にいる必要がある）
