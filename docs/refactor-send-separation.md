# リファクタ指示: 処理と送信の分離

## 目的

各サービスの `handle` が「計算」と「Discord への送信」を両方やっている現状を、
**処理（何を返すか）と送信（どう送るか）を分離**する設計に変える。

元の Python 実装がこの分離をしていた（`RCReply` を返す処理系と、
`actionToDiscord` / `sendToDiscord` / `replyToDiscord` の送信系が別）。
その設計思想に寄せる。

### なぜやるか

- **送信先の柔軟性**: 現状 `msg.channel_id.say(...)` で送信先がハードコード。
  分離すれば同じ結果を DM・リプライ・別チャンネルにも送れる。
- **重複排除**: embed 分割ロジック（1900字・10個ごと）が現状 koukai_kyujin に
  ある。占い館など他サービスでも同じ整形が要るので、1箇所に集約する。
- **テスト容易性**: 処理系が EmbedReply を返すだけになれば、Discord の Context/
  Message なしに「入力→出力」を検証できる。

## 現状（変更前）

`src/bot/services/koukai_kyujin/mod.rs` の `handle` が:
1. OCR で画像→テキスト取得（外部I/O）
2. `process_for_embed` で計算 → EmbedReply
3. `arrangement_chunks` で分割し `send_message` で **自分で送信**

`handler.rs` は `koukai_kyujin::handle(ctx, msg, data).await?` を呼ぶだけ
（戻り値なし）。

## あるべき姿（変更後）

処理を3段に分ける:
- **入力取得（外部I/O）**: OCR / ChatGPT 呼び出し。handle に残す（純粋にできない）
- **計算**: recruit::process_for_embed。既に分離済み、変更不要
- **送信**: 新設する共通関数に集約。handle からは消す

### 変更内容

1. **各 service の `handle` の戻り値を変える**
   - `koukai_kyujin::handle` は送信をやめ、`Result<Option<EmbedReply>, Error>`
     を返す。
   - `Some(reply)` = この reply を送ってほしい / `None` = 何もしない
     （例: 画像添付なしで対象外だったケースは `None`）。
   - OCR 失敗などのエラー表示も EmbedReply（エラー用）にして返す。
     EmbedReply に `error(msg: &str) -> EmbedReply` のようなコンストラクタを
     追加してよい（msg_type=Err, title="エラー", chunks=[msg]）。

2. **共通送信関数を新設**（Python の actionToDiscord 相当）
   - 置き場所: `src/bot/mod.rs` か、新規 `src/bot/reply.rs` のどちらか適切な方。
   - シグネチャ例:
     ```rust
     pub async fn send_embed_reply(
         ctx: &serenity::Context,
         channel_id: serenity::ChannelId,
         reply: &EmbedReply,
     ) -> Result<(), Error>
     ```
   - 現状 koukai_kyujin/mod.rs にある embed 分割・送信ロジック
     （arrangement_chunks で 1900字詰め直し → CreateEmbed に title/description/
     colour → 10個ごとに send_message）を **そっくりこの関数に移動**する。
   - EmbedReply の msg_type から colour を取る処理もここに含める。

3. **handler.rs を「呼ぶ→返ってきたら送る」に変更**
   ```rust
   channels::KOUKAI_KYUJIN => {
       if let Some(reply) = koukai_kyujin::handle(ctx, msg, data).await? {
           send_embed_reply(ctx, msg.channel_id, &reply).await?;
       }
   }
   ```
   - uranai も将来同じパターンにできるよう、可能なら handle の戻り値を
     揃えておく（uranai は未実装なので Option<EmbedReply> を返す空実装でよい）。

## 壊してはいけない制約

- **Python 完全一致の出力**: recruit/ 以下（calc/format/matcher）のロジックは
  一切変更しない。今回のリファクタは bot/ の送信構造だけが対象。
- **文字数ベースの分割**: 送信関数に移す分割ロジックは chars().count() ベースの
  まま（バイト数の len() に変えない）。
- **embed の仕様**: 1900字で arrangement_chunks、全 embed に同タイトル・同色、
  10個ごとに分割送信、という挙動を変えない。移動するだけ。
- **外部I/O は handle に残す**: OCR 呼び出しを送信関数側に移そうとしない。
  「入力取得は handle、計算は recruit、送信は共通関数」の3分割を守る。

## 受け入れ条件（完成の判定）

- [ ] `cargo build` が通る
- [ ] `src/bot/services/koukai_kyujin/mod.rs` から send_message / embed 分割
      ロジックが消え、EmbedReply を返すだけになっている
- [ ] 共通送信関数 send_embed_reply が1箇所に存在し、handler.rs がそれを呼ぶ
- [ ] 求人チャンネルに画像を貼ると、リファクタ前と同じ embed 表示が出る
      （タイトル・緑色・コードブロック・1900字分割、すべて従来通り）
- [ ] EmbedReply にエラー用コンストラクタが追加され、OCR 失敗時もそれ経由で
      エラー embed が出る

## 補足

このリファクタは占い館（uranai）実装の**前**にやる意味がある。
分離しておけば、占い館は最初から send_embed_reply を使えて、送信ロジックを
再実装せずに済む。