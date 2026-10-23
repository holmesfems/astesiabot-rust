# api/test_runner（試験手順ランナー）

`/TestRunner`（ja）/ `/TestRunner/en`（en）。手順書の Markdown を読み込み、OK/NG を押すだけで
進められる。CDN 参照ゼロ、進捗はサーバーに送らない。アークナイツ外の単発ツール。
ツール切り替えバー（toolnav.html）は業務で使う想定のため**意図的に include しない**
（バー側の TestRunner チップは残す）。

## 構成

```
mod.rs        … "/"=ja, "/en"=en, "/static"=ServeDir, "/skill.zip"（下記）
templates/    … tr_index.html / tr_index_en.html。静的ラベルは markup に直書き＋SEO タグ。
                末尾は <link> と5行のブートストラップ（言語別 constants を import して boot()）だけ
skill/        … 手順書整形スキル test-procedure-formatter の正本
                （SKILL.md / format.ja.md / format.en.md / validate.mjs の4ファイル）
verify.mjs    … core/ と constants/ の検証（実モジュールを import）
e2e.mjs       … Playwright での実機テスト（ja/en 両方。DOM が絡む変更はこちらで確かめる）
static/
  lz-string.min.js … 1.5.0, MIT。共有 URL `#state=<圧縮JSON>` 用。同一オリジン配信
  style.css        … ja/en 共通
  js/              … ES module。依存の向きは constants ← core ← ui ← main
    main.js        … boot({strings,phrases,samples}) と init() だけ
    constants/     … config.js / i18n.js / {strings,phrases,samples}.{ja,en}.js
    core/          … parser.js / score.js / state.js / io.js（DOM 非依存）
    ui/            … dom.js（ui 内の最下層）/ renderer.js / modal.js / flow.js / effects/
```

## 守ること

- **core/ は DOM 非依存**: `document` を一切参照しない（`verify.mjs` が node だけで動くように）。
  `window` / `localStorage` / `LZString` は `state.js` の保存3関数と `io.js` の共有 URL に限って可。
  `constants/config.js` にも実行時に `window` を評価する値を置かない（core が import している。
  環境フラグは `ui/dom.js` へ）。
- **ui/ の循環 import**: renderer ↔ modal ↔ flow ↔ effects/confetti は画面遷移の都合で相互依存している。
  ESM は関数の循環を扱えるので許容しているが、**import したものをトップレベルで呼ばない**（TDZ で落ちる）。
  `ui/dom.js` は循環に入っていない葉なので、アプリ層のフローを足さない。
- **パーサーは日英のキーワードを両方受ける**: `用語定義|Definitions?|Glossary` /
  `配布物|Attachments?|Downloads?|Assets?` / `ビルド|Build|Version` / 記入モード
  `記入|入力|Enter|Fill ?in|TBD`。エイリアスは **非キャプチャグループ `(?:...)`** で足す
  （`BUILD_LINE_RE` の `m[1]` と `BUILD_INPUT_RE` のグループ1を呼び出し側が使う）。
  `準備するもの` / `試験箇所` / `試験の概要` と表の見出し行は元からパース対象外
  （前置きは本文として表示、表の1行目は無条件スキップ）なので、ここにエイリアスは要らない。
- 節タグは見出し末尾の `【Windows】` と `[Windows]` の両方。行末アンカーなので
  `### See [docs](url)` や `### [Draft] Login` には当たらない（verify.mjs に誤爆ケースあり）。
- **`samples.en.js` に日本語を残さない**: AI 整形プロンプトは「この形式で出せ」という指示なので、
  日本語のマーカーが残ると英語ユーザーに日本語を強いる。verify.mjs がプロンプト内の例を
  パーサーに通して、日英とも解釈できることを確かめている。
- **skill.zip は毎回組み立てる**: `GET /TestRunner/skill.zip` は `skill/` の4ファイルと
  本体の `static/js/core/parser.js` を直接読んで zip にする。だから配布するパーサーが古くならない。
  **`skill/` に parser.js のコピーを置かない**（verify.mjs が非存在を、mod.rs のテストが zip 内の
  parser.js と本体のバイト一致を検査する）。ローカルの `.claude/skills/` は展開して置くだけの
  使い捨てで、`/.claude` は `.gitignore` 済み（デプロイに含まれない）。正本は `skill/`。
  `.gitignore` はこの理由で変えない。
