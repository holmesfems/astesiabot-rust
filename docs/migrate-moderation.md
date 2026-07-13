# 移植指示: モデレーション + 1周年ロール

Python の `ServerModerator` クラス（1ファイル）を Rust に移植する。
元コードは「スパム対策系」と「1周年ロール付与（スパムと無関係）」が混在して
いるので、**2ファイルに分割**して移植する。

参照元 Python: （このプロジェクトに含めた元実装、または会話で共有した
serverModerator 相当のコード）

## ファイル分割

```
src/bot/services/
├── moderation.rs   … 罠チャンネル削除・全体通知検知・レート制御（連投/爆撃）
└── anniversary.rs  … 1周年ロール付与（moderation とは独立した別機能）
```

既存の `spam.rs` は役割が変わる/なくなるので、moderation.rs へ発展的に置き換え
（spam.rs は削除してよい。handler.rs の参照も差し替える）。

## 全体の呼び出し構造

Python は `moderingMSG` が入口で、内部で autoDeletion → autoAnniversary →
レート検知 の順に呼び、「メッセージを破棄すべきか」を bool で返していた。

Rust では handler.rs から2つを呼ぶ:
1. `moderation::handle(...)` → メッセージを削除/処断したか bool を返す
2. `anniversary::handle(...)` → 1周年ロール付与（副作用のみ、戻り値は () でよい）

handler.rs の振り分けは「自分→（ここでmoderation/anniversary）→bot→チャンネル別」
の順。moderation が「処断した（true）」を返したら、以降のチャンネル別処理は
スキップする（return）。

## moderation.rs の中身

Python の3系統をすべて移植する:

### 1. 罠チャンネル自動削除+BAN（autoDeletion / autoBan_inAutoDeletion）
- 環境変数 AUTODEL_1〜AUTODEL_6 のチャンネルID群に一致したら、**管理者含め全員**
  のメッセージを削除。
- そのうえでリンク（下記URL検出）または BANWORDS（"discord.gg", "everyone",
  "peach"）を含むなら BAN。
- ただし**管理者は BAN せず通報のみ**（乗っ取り早期警報）。
- 削除したら true を返す（＝このメッセージは破棄済み扱い、レート検知の対象外）。

### 2. 全体通知検知+BAN（autoNoticeAndBan）
- 罠チャンネル以外での処理。
- 管理者の @everyone は正常系（配信宣伝など）なので**対象外**。
- 非管理者が mention_everyone（@everyone/@here で実通知が飛ぶ）を撃ったら即 BAN。
- BAN したら true。

### 3. レート制御（3層構造を維持）
Python の設計を崩さない。検知・分岐・実行を分ける。

**検知層 detect_rate_spam（純粋ロジック、状態更新はするが処断しない）**
- 戻り値は enum SpamKind { None, MultiChannel, SameChannel }
- 状態2つ（下記「状態管理」）を更新しつつ判定:
  - 同一チャンネル連投: (user, channel) の直近 SAME_CH_WINDOW=10秒 の投稿が
    SAME_CH_THRESHOLD=6 回以上でヒット
  - 複数チャンネル爆撃: user の直近 MULTI_CH_WINDOW=7秒 の投稿の
    distinct channel 数が MULTI_CH_THRESHOLD=3 以上でヒット
  - 両方ヒットなら MultiChannel を優先（より悪質）

**ディスパッチ層 handle_rate_spam（管理者分岐・種類分岐）**
- None なら何もしない
- 管理者は処断せず通報のみ（乗っ取り警報）
- 非管理者は実行層へ（現状は mute+purge。TODO コメントで「誤検知ゼロを確認
  できたら ban に差し替え可能」と残す）

**実行層 mute_and_report / ban_and_report（差し替え可能なアクション）**
- mute_and_report:
  1. 遡及削除 purge_recent を**先に**実行（タイムアウト失敗で早期returnする前に
     削除を済ませる、という Python の順序を守る）
  2. TIMEOUT_DURATION=30分 のタイムアウト付与
  3. 通報
- ban_and_report: 将来の受け皿として用意（delete_message_days=7 で ban）

### 状態管理（重要）
Python の2つの可変 dict を Rust の共有状態にする:
```
multi_ch_history: HashMap<UserId, VecDeque<(Instant, ChannelId)>>
same_ch_history:  HashMap<(UserId, ChannelId), VecDeque<Instant>>
```
- これらは **AppState に持たせる**（bot が読み書きする可変状態）。
  `Arc<Mutex<...>>` で包む。求人データ（読み取り専用）とは性質が違う点に注意。
- AppState に `moderation: ModerationState` のようなフィールドを足し、その中に
  上記2つの Mutex を持たせる形が素直。
- deque の古い要素削除（window 外を popleft）は while ループで front を捨てる。

### クロックの使い分け（Python の意図を守る）
- 検知の時刻計測は **std::time::Instant**（time.monotonic 相当、システム時計
  補正の影響を受けない）。
- purge の「直近N秒」の after 指定だけは **実時計**（serenity の Timestamp /
  chrono）。purge API は実時計でしか範囲指定できないため。この使い分けは意図的。

### purge_recent（自分の直近メッセージ一括削除）
- PURGE_LOOKBACK=15秒 以内の、該当ユーザーのメッセージを対象チャンネルから削除。
- 対象チャンネル: SameChannel は発生チャンネル1つ、MultiChannel は
  multi_ch_history に記録された channel_id 群。
- **serenity には Python の channel.purge に相当する一括APIが無い**。
  channel の直近メッセージを取得 → author_id 一致でフィルタ → bulk 削除
  （delete_messages, 100件まで/14日以内）→ 14日超は個別 delete フォールバック、
  を自前で実装する。権限エラー(Forbidden)や HTTP エラーは握りつぶして続行
  （止血優先、Python と同じ割り切り）。
- VoiceChannel/StageChannel 内テキストも対象に含める（TextChannel だけに絞ると
  VCチャットへのスパムを取りこぼす。Python のコメント参照）。
- 削除後、そのユーザーの履歴（両dict）をクリアする。

### URL検出 / BANWORDS
- Python の _URL_RE（http/https、www、裸ドメイン+TLD+パスを拾う正規表現）を移植。
  fancy-regex か標準 regex か、パターンが先読みを含まないので **標準 regex で可**。
  （求人の matcher とは別。こちらは先読み不要）
- BANWORDS = ["discord.gg", "everyone", "peach"]、message.content.lower() に対して
  含有判定。

### 通報（create_report）
- 通報チャンネル（環境変数で指定）に送る。
- 内容は report 文 + author 名 + content（"." を "_"、"http" を "ht tp" に置換して
  リンクを無害化）+ チャンネルの jump_url。
- 送信は、先にリファクタした `send_embed_reply` があればそれを使ってもよいが、
  Python は plainText で送っていた。plainText 送信の共通関数がまだ無ければ
  単純に channel.say で可。

## anniversary.rs の中身

Python の autoAnniversary を移植。moderation とは完全に独立。
- bot / DM（Member でない）は対象外。
- 環境変数 ANNIROLEID の1周年ロールを既に持っていたら何もしない。
- joined_at から起算し、加入から丸1年以上（Python の条件式そのまま:
  `(now.year - joined.year >= 2) or (now.year - joined.year >= 1 and
  now.month >= joined.month)`）経過していればロール付与。
- joined_at は JST に変換して比較（getnow/JST 相当）。

## 環境変数（.env に追加）
```
AUTODEL_1=... 〜 AUTODEL_6=...   # 罠チャンネルID 6個
ANNIROLEID=...                   # 1周年ロールID
REPORT_CHANNEL_ID=...            # 通報チャンネルID（Python は reportChannel を注入）
```
- moderation は起動時にこれらを読む。値の持ち方（起動時に一度パースして
  ModerationState に持たせる等）は適切に判断してよい。

## 管理者判定
- Python は userIsAdmin を引数で受け取る（呼び出し側が判定済み）。
- Rust では handler 側で、発言者の Member 権限に ADMINISTRATOR が含まれるかを
  判定して moderation に渡す。ギルド外(DM)や Member 取得不可なら false 扱い。

## serenity API について（重要）
副作用の強い操作（delete / ban / timeout / role付与 / bulk削除 / メッセージ取得）
は serenity のバージョンで API 名・シグネチャが変わりやすい。
**実際に cargo build しながら、その環境の serenity の正しい API に合わせること。**
以下は当たりを付けるためのヒント（正確な名前は要確認）:
- メッセージ削除: `msg.delete(&ctx.http).await`
- BAN: `guild_id.ban_with_reason(&ctx.http, user_id, 7, reason).await`
- タイムアウト: Member の communication_disabled_until 系
  （`member.disable_communication_until(...)` 等）
- ロール付与: `member.add_role(&ctx.http, role_id).await`
- 直近メッセージ取得: `channel_id.messages(&ctx.http, builder).await`
- 一括削除: `channel_id.delete_messages(&ctx.http, &ids).await`
- 権限判定: Member の permissions / ロールから ADMINISTRATOR を確認

## 壊してはいけない制約
- **3層構造**（検知/ディスパッチ/実行）を維持。検知層は処断しない。将来 ban へ
  差し替えるとき実行層の呼び先だけ変えれば済む形を保つ。
- **クロックの使い分け**（検知=Instant、purge=実時計）を守る。
- **管理者は原則処断せず通報のみ**（罠チャンネル削除は例外で全員削除）。
- **止血優先の握りつぶし**: purge 中の個別チャンネルのエラーは continue で続行。
- 閾値・ウィンドウ・タイムアウト時間の数値を変えない。

## 受け入れ条件
- [ ] cargo build が通る
- [ ] moderation.rs と anniversary.rs に分かれている
- [ ] spam.rs が置き換え/削除され、handler.rs が新しい2つを呼んでいる
- [ ] AppState にモデレーション用の可変状態（Arc<Mutex>）が追加されている
- [ ] レート制御が3層構造を保っている（detect/dispatch/execute が分離）
- [ ] 検知は Instant、purge の範囲指定は実時計になっている
- [ ] 罠チャンネル削除・全体通知BAN・連投/爆撃検知・1周年ロールの4系統が揃う
- [ ] 環境変数（AUTODEL_1..6 / ANNIROLEID / REPORT_CHANNEL_ID）を読んでいる

## 動作確認の注意
実際の BAN/タイムアウトはテストしづらい（本番影響が大きい）。まずは cargo build
を通し、レート制御のしきい値ロジックが意図通りかは、可能なら detect 層だけを
切り出した単体テスト（Instant を差し込んで連投をシミュレート）で確認するとよい。
実サーバーでの BAN 挙動テストは慎重に（テスト用アカウント/権限で）。