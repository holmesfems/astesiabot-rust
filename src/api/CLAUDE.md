# api（axum の web API と Web UI）

ツールごとの詳細は各フォルダの CLAUDE.md（`fk_kill_calculator/`）。

## ルーティング

- `mod.rs`: `AppState`、`run_api`、`base_url()`、`/robots.txt`・`/sitemap.xml`、`web_ui_router()`。
- **UI ルートは `web_ui_router()` に足す**。`run_api` に直接書くと dev 用の `serve_web` と `e2e.mjs`
  から見えなくなり、dev と本番でルートがずれる。`run_api` はそれに状態が要るルート
  （`/recruitment/`、`/api/risei/*`、FK の catalog.json）と SwaggerUi を足すだけにする。
- **絶対 URL（canonical/hreflang/sitemap）は `base_url()` に集約**: `PUBLIC_BASE_URL`（任意）を優先し、
  無ければ `X-Forwarded-Proto` + `Host`（Heroku 等は手前で TLS を終端するので、scheme をヘッダから
  見ないと http になる）。個別にホスト名を書かない。
- **旧ホストのリダイレクトは GET/HEAD だけ**（`legacy_host_redirect.rs`。`*.herokuapp.com` と
  `www.` → `PUBLIC_BASE_URL` へ 301。未設定なら無効。run_api のみ）。POST をリダイレクトすると
  クライアントが GET に変えて本文を捨てる。`/recruitment/` と `/health` はパスでも除外。
  axum の `Redirect::permanent` は 308 なので 301 は自前で組んでいる。

## エンドポイント

- `recruitment.rs` … `POST /recruitment/`（iOS ショートカット等から叩く公開求人 API）。
  応答に `tagCount` と、`shortcutVersion` が `data/recruitment/shortcut.yaml` の最新より古い時だけ
  更新通知 `update` を付ける（版を送らない旧ショートカットには `reply` 末尾にも通知文。
  `latest_version` が0の間は出さない）。`pickupOperators` は入力しづらい別名を正式表記へ寄せる
  （`PICKUP_ALIASES`。例: `GALLUS2` → `GALLUS²`）。認証なしなので入口で量を絞る:
  `text` は `MAX_TEXT_CHARS`（2000文字。超えたら400＋通常形のエラー）、本文は `MAX_BODY_BYTES`
  （64KiB。このルートだけ DefaultBodyLimit、超えたら413）。入手用固定 URL
  `GET /shortcut/recruitment`（iCloud リンクへ307。`shortcut_router()`、web_ui_router に載せる）。
- `risei/` … `GET /api/risei/*`（理性価値計算と育成コストの REST API。認証なし公開。
  理性効率動画づくりで Claude が叩く）。engine の DTO をそのまま JSON で返し、エラーは常に JSON で
  `did_you_mean` / `usage` を添える。`GET /api/risei` が使い方一覧（`ENDPOINTS` 表）。
  `router(RiseiApiState { risei, external_source })` を run_api が merge する（serve_web には無い）。
  `operators/*` は `server` 引数を持たず、bot と同じく `cn_only` で換算サーバを切り替える。
  `lists/kakin` はグローバル版のみ（mainland は400）で、`baselines` / `packs` を返す
  （計算は `engine/risei_calculator_engine/kakin.rs` を bot と共有）。
- `risei_skill/SKILL.md` … API の使い方スキルの正本。`GET /api/risei/skill.zip`（`skill_router()`。
  状態不要なので web_ui_router に載せる）がリクエスト毎に zip にする。

## Web UI 共通

- `home/` がトップページ、`templates_shared/` が共通パーツ（`toolnav.html` / `head_icons.html`
  ＝全ページの `<head>` に必須 / `og_square_image.html` ＝ home 以外が使う正方形 og:image。
  include 先の struct に `base` が要る）。
- **askama の `dirs` はフラットに解決される**ので、テンプレート名はモジュールごとに前置する
  （`ef_*` / `wl_*` / `lod_*` / `fkc_*` / `home_*`）。素の `index.html` などは他と衝突する。
- **lod_chest_solver は言語ごとに別 URL・別 HTML**（1URL=1言語）。クローラに両言語の
  中身を見せるため、静的ラベルは各言語テンプレートに直書きし、JS で差し替えない
  （localStorage / navigator.language / Accept-Language での振り分けもしない。重複扱いされ得る）。
  計算層・表現層・CSS は言語間で共有し、差は文言データだけにする
  （`engine.js` ＋ `initUi(strings)`）。
- **サイトアイコンは生成物**: 元画像は `assets/icon/`（通常版 PNG と simple 版 SVG）。差し替えたら
  `& "C:\Program Files\nodejs\node.exe" assets/icon/generate.mjs` で `site_icons/static/` を作り直す
  （手で編集しない）。48px 以下と maskable は simple 版、180px 以上は通常版。simple 版の星は
  `<use fill="…">` なので、ブラウザ以外のラスタライザだと黒くなる。PNG 化は必ず generate.mjs 経由。
  新しいページは `head_icons.html` を include し、`mod.rs` の `every_page_links_site_icons` にも足す。
- **home の星データは生成物**: `home/static/stars.json` と `zodiac_lines.json`（黄道十二星座の線）は
  `assets/stars/generate.mjs` で作る（手で編集しない。線の結び方は generate.mjs の `ZODIAC` に一から書き起こしたもの）。
  `planisphere.js` は早見盤（天の北極中心の正距方位図法）で、星空は一辺 2R の正方形の canvas（`#star-canvas`。外側が `#sky`）に
  回転0で1回だけ描き（リサイズのときだけ描き直す）、スクロールに合わせた回転は CSS の `rotate` で `#sky` と大きな fixed ダイヤル
  （`#dial` の `#dial-ring`。北極＝その中心。黄道十二星座の記号つきの金の環）を回す（再描画なし。スマホの慣性スクロールのガクつき対策）。
  `@supports (animation-timeline: scroll())` ならスクロール駆動アニメーション（JS は `--rot-max` を入れるだけ）、
  非対応なら JS が scroll で `style.rotate` を入れる。どちらも角度は scrollY×0.06deg（時計回り）。
  ダイヤルはカバーの背景と文字の間に敷き（`pointer-events:none`・掴めない）、下へスクロールすると時計回りに回って内容と同じ向きに動く。
  重ね順は `home_index.html` の CSS コメント。
  ダイヤルの中心は本文カバー（`main.plate`）の右辺に固定（スマホは全幅なので画面の右端）で、位置と直径は JS（`layout()`）が入れる。
  オープニング（`opening.js`）の再生中は、ダイヤルと北極をエンブレム（`.opening-stage`）に重ねて置く（`--open-tx/ty/s` の FLIP）。
  自然に終わると `html.opening-done` で CSS がダイヤルと空を本来の位置へ滑らせ、スキップ（クリック・キー）はクラスを外すだけでその場で最終状態になる。
  両 JS は `window` の `home-opening-end` イベントで緩く連携する。
- **ef_recipe_calculator は計算をブラウザでする**（サーバーは計算しない。issue #69）。計算層 `static/solver.js`
  （アルゴリズムは `EFRecipeCalculator.md` §4）、表示層 `static/render.js`、エディタ `static/app.js`。
  ページは `app.js?v=N` で読む（旧版のキャッシュ対策。app.js の作りを変えたら N を上げる）。
- **wl_battery_simulator も計算をブラウザでする**（issue #69）。計算層 `static/optimizer.js`（入口）と
  `static/battery_sim.js`、表示層 `static/render.js`（Chart.js は CDN のグローバル）、ページの処理 `static/app.js`
  （`?v=N` は EF と同じ）。`testdata/golden.json` は移す前の Rust 版の出力で、verify.mjs が突き合わせる
  （計算を意図して変えたときだけ作り直す）。可変周期入力式標準制御器（CTL_2）は不安定なので外した。
