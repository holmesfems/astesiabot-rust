// 武陵発電計算機の表現層（ブラウザ実機）を Playwright で検証するスクリプト。
// verify.mjs（計算層。DOM非依存）と対になる。ef_recipe_calculator/e2e.mjs と同じ枠組み。
//
// 実行方法（node は PATH に無いが実体は存在する。絶対パスで呼ぶこと）:
//   "C:\Program Files\nodejs\node.exe" src/api/wl_battery_simulator/e2e.mjs
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
// このファイルは src/api/wl_battery_simulator/e2e.mjs なので、リポジトリルートは3階層上。
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

// ページを開き、エラーと計算リクエストを記録する。計算はブラウザだけで完結し、
// /calculate へは1件も飛ばないはず。
async function openPage(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const log = { consoleErrors: [], pageErrors: [], calcRequests: [] };
  page.on('console', (msg) => { if (msg.type() === 'error') log.consoleErrors.push(msg.text()); });
  page.on('pageerror', (err) => log.pageErrors.push(String(err && err.stack ? err.stack : err)));
  page.on('request', (req) => { if (req.url().includes('/calculate')) log.calcRequests.push(req.method() + ' ' + req.url()); });
  await page.goto(baseUrl + '/WLBatterySimulator', { waitUntil: 'networkidle' });
  return { context, page, log };
}

function checkLog(prefix, log) {
  ok(prefix + ': no page errors', log.pageErrors.length === 0, log.pageErrors.join(' | '));
  ok(prefix + ': no console errors', log.consoleErrors.length === 0, log.consoleErrors.join(' | '));
  ok(prefix + ': no request to /calculate', log.calcRequests.length === 0, log.calcRequests.join(' | '));
}

async function calc(page, requiredPower) {
  await page.fill('#required_power', String(requiredPower));
  await page.click('#calcForm button[type="submit"]');
}

// Chart.js のインスタンスがあり、点が描かれているか。
async function chartPoints(page) {
  return page.evaluate(() => {
    const canvas = document.querySelector('#tvChart canvas#chart');
    const chart = canvas && window.Chart && window.Chart.getChart(canvas);
    return chart ? chart.data.datasets[0].data.length : -1;
  });
}

async function runMainScenario(browser, baseUrl) {
  const { context, page, log } = await openPage(browser, baseUrl);
  try {
    ok('page has title', (await page.title()).includes('武陵'));
    ok('result hidden before calculating', await page.locator('#result').isHidden());
    ok('chart shows the placeholder before calculating',
      (await page.locator('#tvChart').innerText()).includes('計算後に表示されます'));
    ok('CTL_2 is no longer offered', (await page.locator('#blueprintSelector option[value="CTL_2"]').count()) === 0);

    // 既定の図面（CTL_4: 武陵中電池）で計算する。期待値は移す前の Rust 版と同じ（testdata/golden.json）。
    ok('default blueprint is CTL_4', (await page.inputValue('#blueprintSelector')) === 'CTL_4');
    await calc(page, 230);
    const result = await page.locator('#result').innerText();
    ok('result shown', await page.locator('#result').isVisible());
    ok('CTL_4 230: required power and setting', result.includes('要求電力: 230') && result.includes('推奨発電量: 297.67'), result);
    ok('CTL_4 230: clock setting', result.includes('周期設定: 430s'), result);
    ok('CTL_4 230: lowest storage and battery saving',
      result.includes('残電最小値: 10300') && result.includes('電池節約: 1959.07/日'), result);
    const parts = await page.locator('#result li img').evaluateAll((imgs) => imgs.map((i) => i.getAttribute('src').split('/').pop()));
    ok('CTL_4 230: circuit parts are battery + merger/crosser for bit_str "0101001"',
      JSON.stringify(parts) === JSON.stringify(['battery.png', 'merger.png', 'crosser.png', 'merger.png', 'crosser.png', 'merger.png', 'merger.png', 'crosser.png']),
      parts.join(','));
    ok('CTL_4 mergers are rotated (right-facing)',
      await page.locator('#result img[src$="merger.png"]').first().evaluate((i) => i.style.transform === 'rotate(180deg)'));
    ok('chart drawn for CTL_4 230', (await chartPoints(page)) === 5, await chartPoints(page));

    // 図面を切り替えると、入力済みの必要電力で計算し直す（CTL_1: PWM回路）。
    await page.selectOption('#blueprintSelector', 'CTL_1');
    const result1 = await page.locator('#result').innerText();
    ok('switching blueprint recalculates (CTL_1 230)', result1.includes('推奨発電量: 250') && !result1.includes('周期設定'), result1);
    ok('CTL_1 mergers are not rotated',
      await page.locator('#result img[src$="merger.png"]').first().evaluate((i) => i.style.transform === ''));
    ok('chart redrawn for CTL_1 (65 points)', (await chartPoints(page)) === 65, await chartPoints(page));
    ok('blueprint code follows the selection', (await page.locator('#codeValue').innerText()) === 'EFO01iUIAE8OIE29i72U4');

    // 最大発電量の倍数ちょうどなら、制御回路は不要。
    await calc(page, 3200);
    const resultFull = await page.locator('#result').innerText();
    ok('multiple of max power: setting is 1600×2', resultFull.includes('推奨発電量: 1600×2'), resultFull);
    ok('multiple of max power: no circuit parts', (await page.locator('#result img').count()) === 0);
    ok('multiple of max power: chart says no control needed',
      (await page.locator('#tvChart').innerText()).includes('周期制御は不要です'));

    // 最大発電量を超えると按分する。
    await calc(page, 1605);
    ok('over max power: 1600×1+…', (await page.locator('#result').innerText()).includes('推奨発電量: 1600×1+'));

    checkLog('main', log);
  } finally {
    await context.close();
  }
}

async function runErrorScenario(browser, baseUrl) {
  const { context, page, log } = await openPage(browser, baseUrl);
  try {
    await calc(page, 230);
    ok('error scenario: result shown first', await page.locator('#result').isVisible());

    // 5刻みでない値は、ブラウザの入力チェック(step=5)を外して送ると計算側のエラーが出る。
    await page.evaluate(() => document.getElementById('calcForm').setAttribute('novalidate', ''));
    await calc(page, 7);
    const error = await page.locator('#error').innerText();
    ok('non-multiple of 5 shows an input error', (await page.locator('#error').isVisible())
      && error.includes('入力エラー') && error.includes('5刻み'), error);
    ok('result hidden on input error', await page.locator('#result').isHidden());
    ok('chart reset on input error', (await page.locator('#tvChart').innerText()).includes('計算後に表示されます'));

    await page.fill('input[name="storage_margin"]', '2.5');
    await calc(page, 230);
    ok('non-integer margin shows "入力値が不正です"', (await page.locator('#error').innerText()).includes('入力値が不正です'));

    // 直して計算すると、エラーが消えて結果が戻る。
    await page.fill('input[name="storage_margin"]', '10000');
    await calc(page, 230);
    ok('fixing the input hides the error', await page.locator('#error').isHidden());
    ok('fixing the input shows the result again', await page.locator('#result').isVisible());

    checkLog('error', log);
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
