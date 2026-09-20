// 試験手順ランナーの表現層（ブラウザ実機）を Playwright で検証するスクリプト。
// verify.mjs（計算層。DOM非依存）と対になる。
//
// 実行方法（node は PATH に無いが実体は存在する。絶対パスで呼ぶこと）:
//   "C:\Program Files\nodejs\node.exe" src/api/test_runner/e2e.mjs
//
// このスクリプトは自分で `cargo run --quiet --bin serve_web` を spawn し、
// bot も ExternalSourceRegistry も起こさない dev サーバー（Web UIのみ配信）に対して
// Playwright で ja / en 両方を検証する。後始末（サーバー停止）は正常・異常・例外いずれの
// 経路でも必ず行う。
//
// 全部 PASS なら最後に ALL PASS と出る。1件でも落ちれば終了コードが 1 になる。

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';
import net from 'node:net';
import http from 'node:http';
import { spawn, execSync } from 'node:child_process';

const require = createRequire(import.meta.url);

const HERE = path.dirname(fileURLToPath(import.meta.url));
// このファイルは src/api/test_runner/e2e.mjs なので、リポジトリルートは3階層上。
const REPO_ROOT = path.resolve(HERE, '..', '..', '..');

let fail = 0;
function ok(name, cond, extra) {
  console.log((cond ? 'PASS  ' : 'FAIL  ') + name + (extra !== undefined ? '  -> ' + extra : ''));
  if (!cond) fail++;
  return cond;
}

/* ========================================================================= *
 * 1. playwright の解決
 *
 * npx 経由で入っているため node_modules が標準の場所に無い。
 * まず通常の解決を試し、だめなら npx キャッシュを探す（ハッシュ名は決め打ちしない）。
 * ========================================================================= */
function resolvePlaywright() {
  try {
    return require('playwright');
  } catch (e) {
    // フォールスルー
  }

  const npxCacheRoot = path.join(
    process.env.LOCALAPPDATA || path.join(process.env.USERPROFILE || '', 'AppData', 'Local'),
    'npm-cache',
    '_npx'
  );
  if (fs.existsSync(npxCacheRoot)) {
    const hashDirs = fs.readdirSync(npxCacheRoot, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => path.join(npxCacheRoot, d.name, 'node_modules', 'playwright'));
    for (const candidate of hashDirs) {
      if (fs.existsSync(path.join(candidate, 'package.json'))) {
        try {
          return require(candidate);
        } catch (e) {
          // 次の候補へ
        }
      }
    }
  }

  console.error('playwright が見つかりません。');
  console.error('  npx 経由でのインストール例: npx playwright install chromium');
  console.error('  探索したパス: ' + npxCacheRoot);
  process.exit(1);
}

const playwright = resolvePlaywright();

/* ========================================================================= *
 * 2. サーバーの起動と後始末
 * ========================================================================= */
function getFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.listen(0, '127.0.0.1', () => {
      const port = srv.address().port;
      srv.close(() => resolve(port));
    });
    srv.on('error', reject);
  });
}

function waitForHealth(port, timeoutMs) {
  const url = `http://127.0.0.1:${port}/health`;
  const startedAt = Date.now();
  let lastNotice = startedAt;
  return new Promise((resolve, reject) => {
    function attempt() {
      const req = http.get(url, (res) => {
        if (res.statusCode === 200) {
          res.resume();
          resolve();
        } else {
          res.resume();
          retry();
        }
      });
      req.on('error', retry);
      req.setTimeout(2000, () => { req.destroy(); });
    }
    function retry() {
      const elapsed = Date.now() - startedAt;
      if (elapsed > timeoutMs) {
        reject(new Error(`/health が ${timeoutMs}ms 以内に応答しませんでした`));
        return;
      }
      if (Date.now() - lastNotice > 10000) {
        lastNotice = Date.now();
        console.log(`  ...serve_web ビルド/起動待ち中 (${Math.round(elapsed / 1000)}s)`);
      }
      setTimeout(attempt, 500);
    }
    attempt();
  });
}

function killProcessTree(pid) {
  if (!pid) return;
  if (process.platform === 'win32') {
    try {
      execSync(`taskkill /pid ${pid} /T /F`, { stdio: 'ignore' });
    } catch (e) {
      // 既に終了している場合など。無視してよい。
    }
  } else {
    try { process.kill(-pid, 'SIGKILL'); } catch (e) { /* ignore */ }
    try { process.kill(pid, 'SIGKILL'); } catch (e) { /* ignore */ }
  }
}

/* ========================================================================= *
 * core/ (DOM非依存) を直接importして期待値を計算する。
 * ハードコードした項目数に頼らず、実データと突き合わせる（verify.mjsと同じ手法）。
 * ========================================================================= */
const JS_BASE = new URL('./static/js/', import.meta.url).href;
const i18nMod = await import(JS_BASE + 'constants/i18n.js');
const parserMod = await import(JS_BASE + 'core/parser.js');
const samplesJa = (await import(JS_BASE + 'constants/samples.ja.js')).SAMPLES;
const samplesEn = (await import(JS_BASE + 'constants/samples.en.js')).SAMPLES;
// core/parser.js は state を触らないので installI18n なしでも動くが、念のため揃えておく。
const stringsJa = (await import(JS_BASE + 'constants/strings.ja.js')).STRINGS;
const phrasesJa = (await import(JS_BASE + 'constants/phrases.ja.js')).PHRASES;
i18nMod.installI18n({ strings: stringsJa, phrases: phrasesJa, samples: samplesJa });

const expectedA = {
  ja: parserMod.parseProcedure(samplesJa.SAMPLE_A),
  en: parserMod.parseProcedure(samplesEn.SAMPLE_A),
};

/* ========================================================================= *
 * 3. テスト本体
 * ========================================================================= */
async function runLangSuite(browser, baseUrl, lang) {
  const label = `[${lang}]`;
  const path_ = lang === 'ja' ? '/TestRunner' : '/TestRunner/en';
  const consoleErrors = [];
  const pageErrors = [];

  const context = await browser.newContext();
  const page = await context.newPage();
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });
  page.on('pageerror', (err) => {
    pageErrors.push(String(err && err.stack ? err.stack : err));
  });

  try {
    await page.goto(baseUrl + path_, { waitUntil: 'networkidle' });

    // --- boot ---
    const pasteText = await page.locator('#paste-textarea').inputValue();
    ok(`${label} boot: sample loaded into paste-textarea`, pasteText.length > 50, pasteText.length);

    // --- 整形スキルのzipダウンロードリンク（見た目の崩れはブラウザ確認のみで担保。
    //     ここでは href が正しいことだけを見る。中身は mod.rs のテストが担保している） ---
    const skillZipHref = await page.locator('#skill-zip-link').getAttribute('href');
    ok(`${label} skill-zip-link points to /TestRunner/skill.zip`,
      skillZipHref === '/TestRunner/skill.zip', skillZipHref);

    // --- 開始 -> 確認モーダル ---
    await page.click('#paste-start-btn');
    await page.waitForSelector('#modal-confirm:not([hidden])', { timeout: 5000 });
    ok(`${label} modal-confirm opens`, true);

    const placeholder = await page.locator('#tester-name-input').getAttribute('placeholder');
    ok(`${label} tester name placeholder is drawn`, !!placeholder && placeholder.trim() !== '', placeholder);

    // --- OSバッジ (サンプルAは Windows / Android) ---
    const osBadgesText = await page.locator('#os-badges').innerText();
    ok(`${label} os badge shows Windows`, osBadgesText.includes('Windows'), osBadgesText);
    ok(`${label} os badge shows Android`, osBadgesText.includes('Android'), osBadgesText);

    // --- サンプルAは固定ビルドなので build-input は非表示 ---
    ok(`${label} build-input hidden for fixed build (sample A)`,
      !(await page.locator('#build-input').isVisible()));

    // --- サンプルBに切り替えると build-input (記入モード) が表示される ---
    await page.click('#confirm-back-btn');
    await page.waitForSelector('#modal-confirm', { state: 'hidden', timeout: 5000 });
    await page.click('#sample-b-btn');
    await page.click('#paste-start-btn');
    await page.waitForSelector('#modal-confirm:not([hidden])', { timeout: 5000 });
    ok(`${label} build-input visible for input-mode build (sample B)`,
      await page.locator('#build-input').isVisible());

    // --- サンプルAへ戻して実際にセッションを開始する ---
    await page.click('#confirm-back-btn');
    await page.waitForSelector('#modal-confirm', { state: 'hidden', timeout: 5000 });
    await page.click('#sample-a-btn');
    await page.click('#paste-start-btn');
    await page.waitForSelector('#modal-confirm:not([hidden])', { timeout: 5000 });

    // 開始画面で下までスクロールした状態から遷移させ、実行画面が先頭から始まることを見る。
    // showScreen() の window.scrollTo(0,0) が無いとステップカードの上部が見切れる。
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    const scrollBeforeStart = await page.evaluate(() => window.scrollY);
    ok(`${label} scrolled down before starting (前提)`, scrollBeforeStart > 0, scrollBeforeStart);

    await page.click('#confirm-start-btn');
    await page.waitForSelector('#screen-step:not([hidden])', { timeout: 5000 });
    ok(`${label} started -> screen-step`, true);
    ok(`${label} scroll reset to top on screen transition`,
      (await page.evaluate(() => window.scrollY)) === 0,
      await page.evaluate(() => window.scrollY));

    // --- 紙吹雪: OK前は不透明ピクセル0、OKの250ms後は>0 ---
    async function opaquePixelCount() {
      return await page.evaluate(() => {
        const canvas = document.getElementById('confetti-canvas');
        const ctx = canvas.getContext('2d');
        const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
        let count = 0;
        for (let i = 3; i < data.length; i += 4) {
          if (data[i] > 0) count++;
        }
        return count;
      });
    }
    const beforeOk = await opaquePixelCount();
    ok(`${label} confetti canvas empty before OK`, beforeOk === 0, beforeOk);

    await page.click('#ok-btn');
    await page.waitForTimeout(250);
    const afterOk = await opaquePixelCount();
    ok(`${label} confetti canvas has particles 250ms after OK`, afterOk > 0, afterOk);

    const praiseShown = await page.locator('#praise-pop').evaluate((n) => n.classList.contains('show'));
    ok(`${label} praise-pop shown after OK`, praiseShown);

    // OK直後の遷移アニメ(480ms)が終わるまで待ってから次項目を操作する
    await page.waitForTimeout(800 - 250);

    // --- 逃げるNGボタン: 1回目・2回目は動き、3回目(NG_DODGE_LIMIT=2)は動かない ---
    const ngBtn = page.locator('#ng-btn');
    async function ngBtnX() {
      const box = await ngBtn.boundingBox();
      return box.x;
    }
    const dodgeMoved = [];
    let bubbleSeenDuringDodge = 0;
    for (let i = 0; i < 3; i++) {
      const xBefore = await ngBtnX();
      await page.mouse.move(5, 5);
      const box = await ngBtn.boundingBox();
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.waitForTimeout(900);
      // 吹き出しはbubble生成(移動から380ms後)から900ms後に自動で消えるので、
      // 静止待ちの直後（まだ消える前）にここで数える。ループの最後まで待つと
      // 直近の吹き出しが既に消えて0件になってしまう（実際に踏んだ自分のテストバグ）。
      bubbleSeenDuringDodge += await page.locator('#ng-btn .ng-bubble').count();
      const xAfter = await ngBtnX();
      dodgeMoved.push(Math.abs(xAfter - xBefore) > 1);
    }
    ok(`${label} ng-btn dodges on hover #1`, dodgeMoved[0] === true, dodgeMoved);
    ok(`${label} ng-btn dodges on hover #2`, dodgeMoved[1] === true, dodgeMoved);
    ok(`${label} ng-btn stays put on hover #3 (NG_DODGE_LIMIT=2)`, dodgeMoved[2] === false, dodgeMoved);
    ok(`${label} ng-btn shows a dodge bubble`, bubbleSeenDuringDodge > 0, bubbleSeenDuringDodge);

    // --- NG引き止めフロー ---
    await ngBtn.click();
    await page.waitForSelector('#overlay-ng-confirm:not([hidden])', { timeout: 5000 });
    const deterrentText = await page.locator('#ng-confirm-deterrent').innerText();
    ok(`${label} ng-confirm overlay shows deterrent text`, deterrentText.trim().length > 0, deterrentText);

    await page.click('#ng-confirm-proceed-btn');
    await page.waitForSelector('#ng-panel:not([hidden])', { timeout: 5000 });
    ok(`${label} ng-confirm-proceed-btn opens ng-panel`, true);

    ok(`${label} ng-confirm-btn disabled while comment empty`,
      await page.locator('#ng-confirm-btn').isDisabled());
    await page.fill('#ng-comment', 'e2e: expected result did not match');
    ok(`${label} ng-confirm-btn enabled once comment is filled`,
      !(await page.locator('#ng-confirm-btn').isDisabled()));

    await page.click('#ng-confirm-btn');
    await page.waitForTimeout(300); // NG後の遷移は60ms

    // --- 残りは全部OKで進めて結果画面まで到達する ---
    for (let guard = 0; guard < 200; guard++) {
      if (await page.locator('#screen-result').isVisible()) break;
      if (await page.locator('#overlay-section-complete').isVisible()) {
        await page.click('#sc-next-btn');
        await page.waitForTimeout(300);
        continue;
      }
      if (await page.locator('#overlay-finale').isVisible()) {
        await page.click('#finale-result-btn');
        await page.waitForTimeout(300);
        continue;
      }
      if (await page.locator('#ng-panel').isVisible()) {
        // このループに入る時点では記録済みのはずだが、保険として抜ける
        break;
      }
      const okBtnLocator = page.locator('#ok-btn');
      if (await okBtnLocator.isEnabled()) {
        await okBtnLocator.click();
        await page.waitForTimeout(550);
      } else {
        await page.waitForTimeout(200);
      }
    }
    await page.waitForSelector('#screen-result:not([hidden])', { timeout: 10000 });
    ok(`${label} reached screen-result`, true);
    // 結果画面は縦に長いので、途中から表示されると総合ランクやサマリーを見落とす
    ok(`${label} scroll reset to top on reaching the result screen`,
      (await page.evaluate(() => window.scrollY)) === 0,
      await page.evaluate(() => window.scrollY));

    const rowCount = await page.locator('#result-table-body tr').count();
    const expectedTotal = expectedA[lang].totalItems;
    ok(`${label} result table row count matches totalItems`, rowCount === expectedTotal,
      `${rowCount} vs ${expectedTotal}`);

    // --- console/page エラーは最後にまとめて検証 ---
    ok(`${label} no pageerror events`, pageErrors.length === 0, pageErrors.join(' | '));
    ok(`${label} no console.error events`, consoleErrors.length === 0, consoleErrors.join(' | '));
  } catch (e) {
    fail++;
    console.log(`FAIL  ${label} unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, `e2e_fail_${lang}.png`);
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

/* ========================================================================= *
 * 3b. ラベル注入の踏み抜き防止（パーサー警告ワークストリーム設計書 2.5）
 *
 * core/parser.js は i18n 依存を外し、呼び出し側が T（strings.{ja,en}.js）を渡す形に
 * なった。渡し忘れると「英語ページなのに期待結果欄に日本語の（記載なし）が出る」という
 * 静かな不具合になる。verify.mjs は DOM を触らずに parseProcedure の戻り値だけを見て
 * いるので、この不具合そのものは実ブラウザで実際にレンダリングしてみないと踏めない。
 * ========================================================================= */
async function runLabelInjectionCheck(browser, baseUrl, lang) {
  const label = `[${lang}] label-injection`;
  const path_ = lang === 'ja' ? '/TestRunner' : '/TestRunner/en';
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.goto(baseUrl + path_, { waitUntil: 'networkidle' });

    // 期待結果の列をわざと省いた最小の手順書。2列の表なので expectedRaw が
    // notSpecified（ページの言語の T.notSpecified）で埋まる。
    const minimalMd = [
      '## Label injection check',
      '### 1. S',
      '',
      '| No | Step |',
      '|---|---|',
      '| 1 | do it |'
    ].join('\n');

    await page.fill('#paste-textarea', minimalMd);
    await page.click('#paste-start-btn');
    await page.waitForSelector('#modal-confirm:not([hidden])', { timeout: 5000 });
    await page.click('#confirm-start-btn');
    await page.waitForSelector('#screen-step:not([hidden])', { timeout: 5000 });

    const expectedText = (await page.locator('#item-expected-body').innerText()).trim();
    const want = lang === 'ja' ? '（記載なし）' : '(not specified)';
    ok(`${label}: expected-result column shows the page's own language`, expectedText === want, expectedText);
  } catch (e) {
    fail++;
    console.log(`FAIL  ${label} unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, `e2e_fail_label_${lang}.png`);
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

/* ========================================================================= *
 * main
 * ========================================================================= */
let serverProc = null;
let browser = null;

// 後始末。正常終了・例外・Ctrl+C のどれを通っても必ず serve_web を落とす。
// これを取りこぼすと cargo.exe と serve_web.exe が孤児として残り、
// 次の `cargo build` が target/debug/serve_web.exe を置き換えられずに
// os error 5 でこける（原因が直前のCtrl+Cだと気づきにくい）。
// 何度呼ばれても安全なようにフラグで1回に絞る。
let cleanedUp = false;
function cleanup() {
  if (cleanedUp) return;
  cleanedUp = true;
  if (serverProc && serverProc.pid) killProcessTree(serverProc.pid);
}

// 'exit' は同期処理しか走らせられないが、killProcessTree は同期なので問題ない。
// 未捕捉例外で落ちるときもここを通る。
process.on('exit', cleanup);
// Windows の Ctrl+C は SIGINT、Ctrl+Break は SIGBREAK で届く。
for (const sig of ['SIGINT', 'SIGTERM', 'SIGBREAK']) {
  process.on(sig, () => {
    cleanup();
    process.exit(130);
  });
}

try {
  const port = await getFreePort();
  const baseUrl = `http://127.0.0.1:${port}`;
  console.log(`serve_web を起動します (WEB_UI_PORT=${port}, cwd=${REPO_ROOT})`);

  serverProc = spawn('cargo', ['run', '--quiet', '--bin', 'serve_web'], {
    cwd: REPO_ROOT,
    env: { ...process.env, WEB_UI_PORT: String(port) },
    stdio: ['ignore', 'inherit', 'inherit'],
    shell: process.platform === 'win32',
  });

  await waitForHealth(port, 240000);
  console.log('serve_web is up.');

  browser = await playwright.chromium.launch();

  await runLangSuite(browser, baseUrl, 'ja');
  await runLangSuite(browser, baseUrl, 'en');
  await runLabelInjectionCheck(browser, baseUrl, 'ja');
  await runLabelInjectionCheck(browser, baseUrl, 'en');
} catch (e) {
  fail++;
  console.log('FAIL  fatal -> ' + (e && e.stack ? e.stack : e));
} finally {
  if (browser) {
    try { await browser.close(); } catch (e) { /* ignore */ }
  }
  cleanup();
}

console.log(fail === 0 ? '\nALL PASS' : '\n' + fail + ' FAILURES');
if (fail > 0) process.exit(1);
