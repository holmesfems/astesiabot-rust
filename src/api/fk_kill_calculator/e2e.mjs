// フレームキル計算機の表現層（ブラウザ実機）を Playwright で検証するスクリプト。
// verify.mjs（計算層。DOM非依存）と対になる。test_runner/e2e.mjs と同じ枠組み。
//
// 実行方法（node は PATH に無いが実体は存在する。絶対パスで呼ぶこと）:
//   "C:\Program Files\nodejs\node.exe" src/api/fk_kill_calculator/e2e.mjs
//
// このスクリプトは自分で `cargo run --quiet --bin serve_web` を spawn し、
// bot も ExternalSourceRegistry も起こさない dev サーバーに対して Playwright で検証する。
// 後始末（サーバー停止）は正常・異常・例外いずれの経路でも必ず行う。

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';
import net from 'node:net';
import http from 'node:http';
import { spawn, execSync } from 'node:child_process';

const require = createRequire(import.meta.url);
const HERE = path.dirname(fileURLToPath(import.meta.url));
// このファイルは src/api/fk_kill_calculator/e2e.mjs なので、リポジトリルートは3階層上。
const REPO_ROOT = path.resolve(HERE, '..', '..', '..');

let fail = 0;
function ok(name, cond, extra) {
  console.log((cond ? 'PASS  ' : 'FAIL  ') + name + (extra !== undefined ? '  -> ' + extra : ''));
  if (!cond) fail++;
  return cond;
}

/* ========================================================================= *
 * 1. playwright の解決（test_runner/e2e.mjsと同じ。npxキャッシュも探す）
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
 * 3. テスト本体
 * ========================================================================= */
async function selectEntryByLabel(page, substring) {
  const select = page.locator('select[data-field="entryIdx"]');
  const labels = await select.locator('option').allTextContents();
  const idx = labels.findIndex((l) => l.includes(substring));
  if (idx < 0) throw new Error(`entry containing "${substring}" not found in [${labels.join(', ')}]`);
  await select.selectOption(String(idx));
  return idx;
}

async function addOperator(page, name, entrySubstring) {
  await page.click('#add-row-btn');
  await page.waitForTimeout(50);
  const nameInput = page.locator('[data-field="opName"]');
  await nameInput.fill(name);
  await nameInput.press('Tab');
  await page.waitForTimeout(100);
  if (entrySubstring) await selectEntryByLabel(page, entrySubstring);
  await page.waitForTimeout(100);
}

async function runMainScenario(browser, baseUrl) {
  const consoleErrors = [];
  const pageErrors = [];
  // 「共有URLをコピー」の中身をクリップボードから読んで検証するため権限を付ける。
  const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await context.newPage();
  page.on('console', (msg) => { if (msg.type() === 'error') consoleErrors.push(msg.text()); });
  page.on('pageerror', (err) => pageErrors.push(String(err && err.stack ? err.stack : err)));

  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });

    ok('page has title', (await page.title()).includes('フレームキル計算機'));

    // --- カタログ読み込み ---
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });
    ok('catalog loaded (operator datalist has options)',
      (await page.locator('#operator-datalist option').count()) > 50);

    // --- 開いた直後(未入力)はURLに#state=を書かない（ページ自体を共有しやすくするため） ---
    await page.waitForTimeout(500); // saveToHashのデバウンス(300ms)より長く待つ
    const hashOnLoad = await page.evaluate(() => location.hash);
    ok('URL has no #state= right after opening (empty state)', hashOnLoad === '', hashOnLoad);
    ok('URL has no trailing "#" either', !(await page.evaluate(() => location.href)).endsWith('#'));

    // --- Ash を追加してS3/400%を選び、敵HPを設定すると判定が更新される ---
    await addOperator(page, 'Ash', '400%');
    await page.fill('#enemy-hp', '5000');
    await page.fill('#enemy-def', '0');
    await page.waitForTimeout(150);
    const verdict1 = await page.locator('#verdict-text').innerText();
    ok('verdict updates after enemy hp/def input', verdict1.includes('撃破できる') || verdict1.includes('足りない'), verdict1);

    // --- 実キー入力: 1打鍵ずつ打っても桁順が保たれ、途中の0も打てる ---
    // （fill()は値を一括で流し込むため、打鍵ごとの再描画でカーソルが先頭へ戻る不具合を
    // 検出できない。ここは必ず実キー入力で確認する）
    await page.fill('#enemy-def', '');
    await page.locator('#enemy-def').pressSequentially('110');
    ok('typing "110" key by key into enemy def yields 110', (await page.inputValue('#enemy-def')) === '110',
      await page.inputValue('#enemy-def'));
    await page.fill('#enemy-hp', '');
    await page.locator('#enemy-hp').pressSequentially('100');
    ok('typing "100" key by key (containing 0s) into enemy hp yields 100', (await page.inputValue('#enemy-hp')) === '100',
      await page.inputValue('#enemy-hp'));
    await page.waitForTimeout(100);
    ok('verdict reflects key-by-key typed hp', (await page.locator('#verdict-text').innerText()).includes('/ 100'),
      await page.locator('#verdict-text').innerText());
    await page.locator('input[data-field="hits"]').fill('');
    await page.locator('input[data-field="hits"]').pressSequentially('10');
    ok('typing "10" key by key into an expanded row field yields 10', (await page.locator('input[data-field="hits"]').inputValue()) === '10',
      await page.locator('input[data-field="hits"]').inputValue());
    ok('expanded row formula updates live while typing', (await page.locator('.row-formula').innerText()).includes('× 10Hit'),
      await page.locator('.row-formula').innerText());
    await page.locator('input[data-field="hits"]').fill('1');
    await page.fill('#enemy-hp', '5000');
    await page.fill('#enemy-def', '0');
    await page.waitForTimeout(150);

    // --- 折りたたんで2人目を追加 ---
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);
    const firstSummary = await page.locator('.row-summary-name').first().innerText();
    ok('first row summary mentions Ash', firstSummary.includes('Ash'), firstSummary);
    ok('collapsed row summary uses short skill ref (S3/400%), not the skill display name',
      firstSummary.includes('S3/400%') && !firstSummary.includes('ブリーチング弾'), firstSummary);

    // --- UIラウンド2: 色ドット(凡例兼用)・ダメージ強調・編集ボタンのテキスト化 ---
    ok('collapsed row has a color dot (legend)', (await page.locator('.row-card').first().locator('.row-color-dot').count()) === 1);
    const firstDamageText = await page.locator('.row-summary-damage').first().innerText();
    ok('collapsed row shows a bold damage figure, never truncated', /^[0-9,]+$/.test(firstDamageText.trim()), firstDamageText);
    const editBtnText = await page.locator('.row-edit-btn').first().innerText();
    ok('edit button uses clear text ("編集"), not the ✎ glyph', editBtnText.trim() === '編集' && !editBtnText.includes('✎'), editBtnText);

    await addOperator(page, 'ブレイズ', null);
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);

    ok('2 row cards after adding second operator', (await page.locator('.row-card').count()) === 2);
    ok('stacked bar has 2 segments', (await page.locator('.bar-seg').count()) === 2);
    const firstSummaryAfter = await page.locator('.row-summary-name').first().innerText();
    ok('first row summary still shows first operator (Ash) after adding a second',
      firstSummaryAfter.includes('Ash'), firstSummaryAfter);
    const hpLabelText = await page.locator('.hp-line-label').innerText();
    ok('bar has an "HP" marker label next to the HP tick', hpLabelText.trim() === 'HP', hpLabelText);

    // --- 行を展開してフィールドを変更 -> 補正バッジ/↺リセットボタンのaria-label ---
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const multiplierInput = page.locator('[data-field="multiplier"]');
    await multiplierInput.fill('9.99');
    await page.waitForTimeout(150);
    const resetBtn = page.locator('[data-action="reset-field"][data-field="multiplier"]').first();
    ok('changed field shows a ↺ reset button', (await resetBtn.count()) === 1);
    const resetAriaLabel = await resetBtn.getAttribute('aria-label');
    ok('↺ reset button has a non-empty aria-label', !!resetAriaLabel && resetAriaLabel.trim().length > 0, resetAriaLabel);
    await resetBtn.click();
    await page.waitForTimeout(100);
    const multiplierAfterReset = await page.locator('[data-field="multiplier"]').inputValue();
    ok('clicking ↺ resets the field back to the catalog default', Number(multiplierAfterReset) !== 9.99, multiplierAfterReset);
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);

    // --- 撃破提案の現実性キャップ: 非現実的な提案の代わりに諦めメッセージが出る ---
    await page.fill('#enemy-hp', '99999999');
    await page.waitForTimeout(150);
    const capMsg = await page.locator('#verdict-suggestions').innerText();
    ok('unrealistic deficits show the "現実的な補正では届きません" fallback instead of absurd suggestions',
      capMsg.includes('現実的な補正では届きません'), capMsg);
    await page.fill('#enemy-hp', '5000'); // 以降のシナリオに影響しないよう戻す
    await page.waitForTimeout(150);

    // --- 複製して削除 ---
    const beforeDup = await page.locator('.row-card').count();
    await page.locator('[data-action="dup-row"]').first().click();
    await page.waitForTimeout(100);
    ok('row count +1 after duplicate', (await page.locator('.row-card').count()) === beforeDup + 1);
    await page.locator('[data-action="del-row"]').first().click();
    await page.waitForTimeout(100);
    ok('row count back to before after delete', (await page.locator('.row-card').count()) === beforeDup);

    // --- 入力しても URL は書き換わらない（状態はlocalStorageに保存） ---
    await page.waitForTimeout(500); // 保存のデバウンス(300ms)より長く待つ
    ok('URL stays without #state= while editing', (await page.evaluate(() => location.hash)) === '');

    // --- リロード -> localStorageから同じ状態に戻る ---
    const rowsBefore = await page.locator('.row-card').count();
    const verdictBefore = await page.locator('#verdict-text').innerText();
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(200);
    ok('row count identical after reload (localStorage)', (await page.locator('.row-card').count()) === rowsBefore);
    ok('verdict text identical after reload (localStorage)',
      (await page.locator('#verdict-text').innerText()) === verdictBefore);

    // --- 共有URLをコピー: #state=付きURLがクリップボードに入り、アドレスバーは変わらない ---
    await page.click('[data-action="share"]');
    await page.waitForTimeout(150);
    const sharedUrl = await page.evaluate(() => navigator.clipboard.readText());
    ok('share copies a URL with #state=', sharedUrl.startsWith(baseUrl + '/FrameKillCalculator#state='), sharedUrl.slice(0, 80));
    ok('share does not change the address bar', (await page.evaluate(() => location.hash)) === '');

    // --- 共有URLを別環境(localStorage空)で開く -> 同じ状態が復元され、#state=は消える ---
    const otherContext = await browser.newContext();
    try {
      const other = await otherContext.newPage();
      await other.setViewportSize({ width: 420, height: 900 });
      await other.goto(sharedUrl, { waitUntil: 'networkidle' });
      await other.waitForTimeout(300);
      ok('shared URL restores the same rows in a fresh browser', (await other.locator('.row-card').count()) === rowsBefore);
      ok('shared URL restores the same verdict', (await other.locator('#verdict-text').innerText()) === verdictBefore);
      ok('#state= is removed from the address bar after loading a shared URL',
        (await other.evaluate(() => location.hash)) === '');
      ok('toast tells the shared state was loaded',
        (await other.locator('.toast').allTextContents()).some((t) => t.includes('共有URLの内容を読み込みました')));
    } finally {
      await otherContext.close();
    }

    // --- 420px幅で横スクロールが出ない ---
    const hasHScroll = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
    ok('no horizontal scroll at 420px width', !hasHScroll);

    // --- 判定セクションはスクロール後も画面内に留まる(sticky/fixed) ---
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(100);
    const verdictVisible = await page.locator('#verdict-section').isVisible();
    const box = await page.locator('#verdict-section').boundingBox();
    const viewportHeight = 900;
    ok('verdict section still visible after scrolling to bottom', verdictVisible);
    ok('verdict section stays within viewport after scroll', box && box.y + box.height <= viewportHeight + 2, box);

    ok('no pageerror events', pageErrors.length === 0, pageErrors.join(' | '));
    ok('no console.error events', consoleErrors.length === 0, consoleErrors.join(' | '));
  } catch (e) {
    fail++;
    console.log(`FAIL  main scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_main.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

async function runStaleRowScenario(browser, baseUrl) {
  // 存在しないopIdを含む状態をURLに埋め込んで開くと、トーストを出しつつ行を除去すること。
  const bootstrapContext = await browser.newContext();
  const bootstrapPage = await bootstrapContext.newPage();
  let hashValue;
  try {
    await bootstrapPage.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    hashValue = await bootstrapPage.evaluate(() => {
      const state = {
        v: 1,
        enemy: { hp: 100, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 },
        rows: [{
          opId: 'char_does_not_exist', entryIdx: 0, dmgType: 'physical', potential: true,
          moduleId: null, moduleLv: 3, multiplier: 1, selfPct: 0, hits: 1, buffPct: 0, dmgMult: 1, ignoreDef: 0,
        }],
      };
      return 'state=' + window.LZString.compressToEncodedURIComponent(JSON.stringify(state));
    });
  } finally {
    await bootstrapContext.close();
  }

  // 同一ドキュメント内でのhash変更(goto->goto)は再ロードにならずinitUi()が再実行され
  // ないため、hash込みの完全なURLへ最初から(新しいコンテキストで)遷移する。
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.goto(baseUrl + '/FrameKillCalculator#' + hashValue, { waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    ok('stale row is dropped on load', (await page.locator('.row-card').count()) === 0);
    const toastTexts = await page.locator('.toast').allTextContents();
    ok('toast is shown for the dropped stale row', toastTexts.some((t) => t.includes('取り除きました')), toastTexts);
  } catch (e) {
    fail++;
    console.log(`FAIL  stale row scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
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
  await runStaleRowScenario(browser, baseUrl);
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
