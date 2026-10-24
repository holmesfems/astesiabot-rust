// エンドフィールド レシピ計算機の表現層（ブラウザ実機）を Playwright で検証するスクリプト。
// verify.mjs（計算層。DOM非依存）と対になる。fk_kill_calculator/e2e.mjs と同じ枠組み。
//
// 実行方法（node は PATH に無いが実体は存在する。絶対パスで呼ぶこと）:
//   "C:\Program Files\nodejs\node.exe" src/api/ef_recipe_calculator/e2e.mjs
//
// このスクリプトは自分で `cargo run --quiet --bin serve_web` を spawn し、
// bot も ExternalSourceRegistry も起こさない dev サーバーに対して Playwright で検証する。
// 後始末（サーバー停止）は正常・異常・例外いずれの経路でも必ず行う。

import { fileURLToPath } from 'node:url';
import path from 'node:path';
import net from 'node:net';
import http from 'node:http';
import { spawn, execSync } from 'node:child_process';
import { resolvePlaywright } from '../e2e_playwright.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
// このファイルは src/api/ef_recipe_calculator/e2e.mjs なので、リポジトリルートは3階層上。
const REPO_ROOT = path.resolve(HERE, '..', '..', '..');

let fail = 0;
function ok(name, cond, extra) {
  console.log((cond ? 'PASS  ' : 'FAIL  ') + name + (extra !== undefined ? '  -> ' + extra : ''));
  if (!cond) fail++;
  return cond;
}

/* ========================================================================= *
 * 1. playwright の解決（ブラウザの入っている版を選ぶ。../e2e_playwright.mjs）
 * ========================================================================= */
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
 * 3. テスト本体
 * ========================================================================= */
const RESULT_CARDS = ['#steps', '#raw-materials', '#mined-usage', '#byproduct-surplus', '#bottleneck', '#warnings'];

// ページを開き、エラーと計算リクエストを記録する。計算はブラウザだけで完結し、
// /calculate へは1件も飛ばないはず。
async function openPage(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const log = { consoleErrors: [], pageErrors: [], calcRequests: [] };
  page.on('console', (msg) => { if (msg.type() === 'error') log.consoleErrors.push(msg.text()); });
  page.on('pageerror', (err) => log.pageErrors.push(String(err && err.stack ? err.stack : err)));
  page.on('request', (req) => { if (req.url().includes('/calculate')) log.calcRequests.push(req.method() + ' ' + req.url()); });
  await page.goto(baseUrl + '/EFRecipeCalculator', { waitUntil: 'networkidle' });
  return { context, page, log };
}

function checkLog(prefix, log) {
  ok(prefix + ': no page errors', log.pageErrors.length === 0, log.pageErrors.join(' | '));
  ok(prefix + ': no console errors', log.consoleErrors.length === 0, log.consoleErrors.join(' | '));
  ok(prefix + ': no request to /calculate', log.calcRequests.length === 0, log.calcRequests.join(' | '));
}

async function loadPreset(page, name) {
  await page.fill('#presetInput', name);
  await page.locator('.combobox-suggestion-item', { hasText: name }).first().dispatchEvent('mousedown');
  await page.click('#loadPresetBtn');
}

async function runMainScenario(browser, baseUrl) {
  const { context, page, log } = await openPage(browser, baseUrl);
  try {
    ok('page has title', (await page.title()).includes('レシピ計算機'));
    for (const id of RESULT_CARDS) {
      ok(`result card ${id} hidden before calculating`, await page.locator(id).isHidden());
    }

    // 焔銅塊チェーン（6レシピ・自己消費あり）を既定の目標で計算する。
    await loadPreset(page, '焔銅塊チェーン');
    ok('preset sets the target item', (await page.inputValue('#targetItemSelect')) === '焔銅塊');
    await page.click('#calcSubmitBtn');

    ok('steps card shown', await page.locator('#steps').isVisible());
    const rowCount = await page.locator('#steps tbody tr').count();
    ok('steps table has 6 rows', rowCount === 6, rowCount);
    const firstRow = await page.locator('#steps tbody tr').first().locator('td').allTextContents();
    ok('step row shows machine count and utilization with %',
      /^\d+$/.test(firstRow[2]) && /^\d+\.\d%$/.test(firstRow[4]), firstRow.join(' / '));
    ok('steps table formats rates as "item: 0.00/min"',
      /: \d+\.\d{2}\/min/.test(await page.locator('#steps tbody').innerText()));
    const raw = await page.locator('#raw-materials').innerText();
    // 期待値は solver.js（= 移す前の Rust 版）がこのプリセットに出す値。
    ok('raw materials list 息壌 and 赤銅塊', raw.includes('息壌: 45.00/min') && raw.includes('赤銅塊: 150.00/min'), raw);
    const bottleneck = await page.locator('#bottleneck').innerText();
    ok('bottleneck names 焔銅塊固体化', bottleneck.includes('焔銅塊固体化'), bottleneck);
    const mined = await page.locator('#mined-usage').innerText();
    ok('mined usage shows used / surplus / cap', mined.includes('息壌ガス: 使用 24.00/min ／ 余剰 0.00/min ／ 上限 24.00/min'), mined);
    ok('warnings card hidden when there are no warnings', await page.locator('#warnings').isHidden());
    ok('error box hidden after a successful calculation', await page.locator('#error').isHidden());

    // 目標レートを変えて計算し直すと結果が置き換わる（追記されない）。
    await page.fill('#targetRateInput', '60');
    await page.click('#calcSubmitBtn');
    ok('recalculating replaces the table (still 6 rows)', (await page.locator('#steps tbody tr').count()) === 6);
    ok('recalculating updates raw materials',
      !(await page.locator('#raw-materials').innerText()).includes('赤銅塊: 150.00/min'));

    checkLog('main', log);
  } finally {
    await context.close();
  }
}

async function runErrorScenario(browser, baseUrl) {
  const { context, page, log } = await openPage(browser, baseUrl);
  try {
    await loadPreset(page, '銅ライン');
    await page.click('#calcSubmitBtn');
    ok('error scenario: results shown first', await page.locator('#steps').isVisible());

    // 目標レートを空にすると入力エラーが出て、前の結果は隠れる。
    await page.fill('#targetRateInput', '');
    await page.click('#calcSubmitBtn');
    const error = await page.locator('#error').innerText();
    ok('empty rate shows an input error', (await page.locator('#error').isVisible()) && error.includes('入力エラー')
      && error.includes('目標レートは正の数で指定してください'), error);
    for (const id of RESULT_CARDS) {
      ok(`result card ${id} hidden on input error`, await page.locator(id).isHidden());
    }

    // 直して計算すると、エラーが消えて結果が戻る。
    await page.fill('#targetRateInput', '30');
    await page.click('#calcSubmitBtn');
    ok('fixing the rate hides the error', await page.locator('#error').isHidden());
    ok('fixing the rate shows results again', await page.locator('#steps').isVisible());

    checkLog('error', log);
  } finally {
    await context.close();
  }
}

async function runWarningScenario(browser, baseUrl) {
  const { context, page, log } = await openPage(browser, baseUrl);
  try {
    await loadPreset(page, '焔銅塊チェーン');
    // 最後のレシピの「使用」を外すと、経路が切れて「原料として扱います」の warning が出る。
    const toggles = page.locator('.recipe-select-toggle input[type="checkbox"]');
    const count = await toggles.count();
    ok('warning scenario: preset has several recipes', count >= 2, count);
    await toggles.nth(count - 1).uncheck();
    await page.selectOption('#targetItemSelect', '焔銅塊');
    await page.click('#calcSubmitBtn');
    ok('warnings card shown', await page.locator('#warnings').isVisible());
    const warnings = await page.locator('#warnings').innerText();
    ok('warning says the item is treated as raw', warnings.includes('原料として扱います'), warnings);

    // レシピ名は HTML として解釈されない（textContent で入れている）。
    await page.locator('.recipe-card-head input[placeholder="レシピ名"]').first().fill('<img src=x onerror=window.__xss=1>');
    await page.click('#calcSubmitBtn');
    ok('recipe names are rendered as text, not HTML',
      (await page.locator('#steps img').count()) === 0
      && (await page.locator('#steps').innerText()).includes('<img src=x')
      && !(await page.evaluate(() => window.__xss)));

    checkLog('warning', log);
  } finally {
    await context.close();
  }
}

/* ========================================================================= *
 * main
 * ========================================================================= */
let serverProc = null;
let browser = null;
let cleanedUp = false;
function cleanup() {
  if (cleanedUp) return;
  cleanedUp = true;
  if (serverProc && serverProc.pid) killProcessTree(serverProc.pid);
}
process.on('exit', cleanup);
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

  await runMainScenario(browser, baseUrl);
  await runErrorScenario(browser, baseUrl);
  await runWarningScenario(browser, baseUrl);
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
