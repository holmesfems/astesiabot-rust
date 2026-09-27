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
    // --- HP未入力(0)では「撃破できる」と出さず入力を促す ---
    const verdictEmpty = await page.locator('#verdict-text').innerText();
    ok('verdict asks for enemy HP while HP is 0 (not "撃破できる")',
      verdictEmpty.includes('敵のHPを入力してください') && !verdictEmpty.includes('撃破できる'), verdictEmpty);

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

// P2: 個別バフ/条件付きバフ/特殊強化トグルの表現層テスト。
// ブレイズ(S3のみのFKエントリ。特殊強化「待機ボーナス」を持つ)とAsh(S3/400%、狙撃=非近距離)を
// 使い、個別バフチップ・全体バフ(条件付き)チップ・特殊強化チェックボックス・single_target警告
// ・条件付き適用状況の✓/–表示・buffN件バッジ・localStorage/共有URLでの永続化を検証する。
async function runBuffScenario(browser, baseUrl) {
  const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    // --- ブレイズを追加(entrySubstring指定なし。S3が唯一のFKエントリ)。
    //     addRow()は追加直後の行を展開状態にするため、ここでは編集ボタンを押す必要は無い。 ---
    await addOperator(page, 'ブレイズ', null);
    await page.waitForSelector('.row-expanded', { timeout: 5000 });

    // --- 特殊強化チェックボックス(P2 follow-up): デフォルトONで、トグルすると加算系
    //     ボーナス(「待機ボーナス6%」)がフォーミュラ行に出たり消えたりする。selfPct自体は
    //     もう変わらない(0.712固定。バグ込みの実測値として常に適用されるため)。---
    const specialCheckbox = page.locator('[data-field="specialOn"]');
    ok('special checkbox (待機ボーナス) is present', (await specialCheckbox.count()) === 1);
    ok('special checkbox is checked by default', await specialCheckbox.isChecked());
    const selfPctInput = page.locator('[data-field="selfPct"]');
    const selfPctBefore = await selfPctInput.inputValue();
    const formulaOn = await page.locator('.row-formula').innerText();
    ok('formula line shows the additive special bonus while ON', formulaOn.includes('待機ボーナス6%'), formulaOn);
    await specialCheckbox.uncheck();
    await page.waitForTimeout(100);
    const selfPctAfterUncheck = await page.locator('[data-field="selfPct"]').inputValue();
    ok(
      'unchecking special toggle does NOT change the selfPct input (fixed 0.712 always)',
      selfPctAfterUncheck === selfPctBefore,
      `${selfPctBefore} -> ${selfPctAfterUncheck}`,
    );
    const formulaOff = await page.locator('.row-formula').innerText();
    ok('formula line hides the additive special bonus while OFF', !formulaOff.includes('待機ボーナス6%'), formulaOff);
    await page.locator('[data-field="specialOn"]').check();
    await page.waitForTimeout(100);
    const formulaRestored = await page.locator('.row-formula').innerText();
    ok('re-checking special toggle restores the bonus in the formula', formulaRestored.includes('待機ボーナス6%'), formulaRestored);

    // --- ⓘボタン: 説明文のトグル(aria-expanded)。P2 follow-up ---
    const infoBtn = page.locator('.special-info-btn');
    ok('ⓘ info button is present next to the special checkbox', (await infoBtn.count()) === 1);
    ok('ⓘ button starts collapsed (aria-expanded=false)', (await infoBtn.getAttribute('aria-expanded')) === 'false');
    ok('special description is hidden before clicking ⓘ', (await page.locator('.special-desc').count()) === 0);
    await infoBtn.click();
    await page.waitForTimeout(100);
    ok('ⓘ button expands (aria-expanded=true) after click', (await page.locator('.special-info-btn').getAttribute('aria-expanded')) === 'true');
    const descText = await page.locator('.special-desc').innerText();
    ok('special description shows the current effective value (現在: +6%)', descText.includes('現在: +6%'), descText);
    await page.locator('.special-info-btn').click();
    await page.waitForTimeout(100);
    ok('ⓘ button collapses again after a 2nd click', (await page.locator('.special-info-btn').getAttribute('aria-expanded')) === 'false');
    ok('special description is hidden again after collapsing', (await page.locator('.special-desc').count()) === 0);

    // --- モジュールを切り替えるとチェックボックス/ヒントが出し分けられる(P2 follow-up)。
    //     ブレイズの特殊強化はモジュールX限定なので、モジュールYへ切り替えるとヒントに
    //     変わり、Xへ戻すとチェックボックスに戻る。 ---
    const moduleSelect = page.locator('select[data-field="moduleId"]');
    const moduleLabels = await moduleSelect.locator('option').allTextContents();
    const yModuleIdx = moduleLabels.findIndex((l) => l.includes('（Y）'));
    const xModuleIdx = moduleLabels.findIndex((l) => l.includes('（X）'));
    ok('module select has both X and Y options for ブレイズ', yModuleIdx >= 0 && xModuleIdx >= 0, moduleLabels);
    if (yModuleIdx >= 0) {
      // moduleOptionsのoption valueはモジュールの実IDなのでindexで選ぶ(entryIdxのような
      // 連番valueではない)。
      await moduleSelect.selectOption({ index: yModuleIdx });
      await page.waitForTimeout(100);
      ok('switching to a non-required module (Y) hides the checkbox', (await page.locator('[data-field="specialOn"]').count()) === 0);
      const hintText = await page.locator('.special-hint').innerText();
      ok('a hint is shown instead, mentioning the required module', hintText.includes('モジュールX'), hintText);
      await moduleSelect.selectOption({ index: xModuleIdx });
      await page.waitForTimeout(100);
      ok('switching back to module X restores the checkbox', (await page.locator('[data-field="specialOn"]').count()) === 1);
    }

    // --- 個別バフチップ(血漿 +90%)をON: フォーミュラ行の「個別」項に反映される ---
    const plasmaChip = page.locator('.chip[data-buff-id="plasma"]');
    ok('individual buff chip for 血漿(plasma) exists', (await plasmaChip.count()) === 1);
    await plasmaChip.click();
    await page.waitForTimeout(100);
    ok('individual buff chip shows aria-pressed=true after click', (await plasmaChip.getAttribute('aria-pressed')) === 'true');
    const formulaWithPlasma = await page.locator('.row-formula').innerText();
    ok('formula line reflects the individual buff pct (個別90%)', formulaWithPlasma.includes('個別90%'), formulaWithPlasma);

    // --- single_targetバフ(アS3/durian。コーディネーター指示でP5にsingle_target化)を
    //     2行で選ぶと⚠が出る ---
    const durianChip1 = page.locator('.chip[data-buff-id="durian"]');
    ok('single_target chip (アS3/durian) exists', (await durianChip1.count()) === 1);
    await durianChip1.click();
    await page.waitForTimeout(100);
    ok('no ⚠ warning yet for durian (selected on only 1 row)', (await durianChip1.locator('.chip-warn').count()) === 0);
    await page.click('[data-action="collapse-row"]');

    // addOperator直後は新しい行(2人目)が展開済み(addRow()の仕様)なので、
    // 編集ボタンを押し直す必要は無い。
    await addOperator(page, 'Ash', '400%');
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const durianChip2 = page.locator('.row-expanded .chip[data-buff-id="durian"]');
    await durianChip2.click();
    await page.waitForTimeout(100);
    ok('durian ⚠ warning appears once the same buff is chosen on a 2nd row',
      (await page.locator('.row-expanded .chip-warn').count()) === 1);
    await durianChip2.click(); // 以降のシナリオに影響しないよう元に戻す(Ash側)
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    await page.locator('.row-expanded .chip[data-buff-id="durian"]').click(); // ブレイズ側も元に戻す
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');

    // --- single_targetバフ(エクシア)は対象人数の拡張(max_targets_by_module。P5)の対象:
    //     育成設定の既定(モジュールX Lv2以上)では2行選んでも⚠が出ない ---
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const exusiaiChip1 = page.locator('.row-expanded .chip[data-buff-id="exusiai"]');
    ok('single_target chip (エクシア) exists', (await exusiaiChip1.count()) === 1);
    await exusiaiChip1.click();
    await page.waitForTimeout(100);
    ok('no ⚠ warning yet (selected on only 1 row)', (await exusiaiChip1.locator('.chip-warn').count()) === 0);
    await page.click('[data-action="collapse-row"]');

    await page.locator('[data-action="edit-row"]').nth(1).click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const exusiaiChip2 = page.locator('.row-expanded .chip[data-buff-id="exusiai"]');
    await exusiaiChip2.click();
    await page.waitForTimeout(100);
    ok('with the default growth setting (module X Lv2+), 2 rows do NOT trigger the ⚠ warning (max_targets_by_module=2)',
      (await page.locator('.row-expanded .chip-warn').count()) === 0);
    await page.click('[data-action="collapse-row"]');

    // --- 3行目でエクシアを選ぶと上限(2)を超えるので⚠が出る ---
    await addOperator(page, 'ブレイズ', null);
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const exusiaiChip3 = page.locator('.row-expanded .chip[data-buff-id="exusiai"]');
    await exusiaiChip3.click();
    await page.waitForTimeout(100);
    ok('⚠ appears once a 3rd row selects エクシア (exceeds max_targets_by_module=2)',
      (await page.locator('.row-expanded .chip-warn').count()) === 1);
    await exusiaiChip3.click(); // 3行目の選択を戻してから削除する
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');
    await page.locator('[data-action="del-row"]').last().click(); // 3行目(検証用に追加したブレイズ)を削除
    await page.waitForTimeout(100);
    ok('back to 2 rows after removing the 3rd', (await page.locator('.row-card').count()) === 2);

    // --- 「個別バフの育成設定」: エクシアのモジュールを「なし」にすると対象人数の上限が
    //     1に戻り、2行選択でも⚠が出るようになる ---
    const exusiaiLevelCard = page.locator('#individual-buff-levels-section .cond-source-card[data-buff-id="exusiai"]');
    ok('individual buff growth settings section shows a card for エクシア (checked on 2 rows)', (await exusiaiLevelCard.count()) === 1);
    const exusiaiValueBefore = (await exusiaiLevelCard.locator('.cond-source-value').innerText()).trim();
    ok('エクシア default resolved value is +10% (E2/潜在6/モジュールXLv3)', exusiaiValueBefore === '+10%', exusiaiValueBefore);
    const exusiaiModuleSelect = exusiaiLevelCard.locator('select[data-field="moduleId"]');
    await exusiaiModuleSelect.selectOption({ label: 'なし' });
    await page.waitForTimeout(100);
    await page.locator('[data-action="edit-row"]').nth(1).click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    ok('⚠ appears on the 2nd row once エクシア growth module is set to なし (limit falls back to 1)',
      (await page.locator('.row-expanded .chip-warn').count()) === 1);
    await page.click('[data-action="collapse-row"]');
    await exusiaiModuleSelect.selectOption({ label: 'X' }); // 元に戻す
    await page.waitForTimeout(100);
    await page.locator('[data-action="edit-row"]').nth(1).click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    ok('⚠ disappears again once module X is reselected', (await page.locator('.row-expanded .chip-warn').count()) === 0);
    await page.click('[data-action="collapse-row"]');

    // --- 全体バフ(条件付き。Castle 近距離+20%)をON: 近距離(ブレイズ)には適用、
    //     非近距離(Ash=狙撃)には適用されない ---
    // ②全体バフは、直前のエクシア育成設定操作(onGlobalBuffLevelChangeがglobalBuffsOpen=trueに
    // する)で既に開いている可能性があるため、summaryクリックは「閉じている場合だけ」行う
    // (開いている状態でクリックするとトグルで閉じてしまう)。
    if (!(await page.locator('#global-buffs-details').evaluate((el) => el.open))) {
      await page.click('#global-buffs-details summary');
      await page.waitForTimeout(50);
    }
    const castleChip = page.locator('.chip[data-buff-id="castle3"]');
    ok('global conditional buff chip (Castle) exists', (await castleChip.count()) === 1);
    await castleChip.click();
    await page.waitForTimeout(150);
    const globalSummaryText = await page.locator('#global-buffs-details summary').innerText();
    ok('global buffs summary shows "1件ON" after toggling Castle on', globalSummaryText.includes('1件ON'), globalSummaryText);

    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const condBlaze = await page.locator('.row-conditional-status').innerText();
    ok('Castle applies (✓) to ブレイズ(近距離)', condBlaze.includes('✓') && condBlaze.includes('Castle'), condBlaze);
    await page.click('[data-action="collapse-row"]');

    await page.locator('[data-action="edit-row"]').nth(1).click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const condAsh = await page.locator('.row-conditional-status').innerText();
    ok('Castle does not apply (–) to Ash(狙撃/非近距離)', condAsh.includes('–') && condAsh.includes('Castle'), condAsh);
    await page.click('[data-action="collapse-row"]');

    // --- P4: Castle(source付き。潜在のみ変化・昇進/モジュールは変わらない)は、
    //     ONにするとインラインの潜在セレクトだけが出て、選択を変えると解決値が変わる ---
    const castleCard = page.locator('.cond-source-card[data-buff-id="castle3"]');
    ok('Castle card has a potential select (potential varies)',
      (await castleCard.locator('select[data-field="potential"]').count()) === 1);
    ok('Castle card has no elite select (elite does not vary; PHASE_0 only)',
      (await castleCard.locator('select[data-field="elite"]').count()) === 0);
    ok('Castle card has no module select (no talent-overriding module)',
      (await castleCard.locator('select[data-field="moduleId"]').count()) === 0);
    const castleValueBefore = (await castleCard.locator('.cond-source-value').innerText()).trim();
    ok('Castle default value is +20% (潜在6)', castleValueBefore === '+20%', castleValueBefore);
    await castleCard.locator('select[data-field="potential"]').selectOption('0');
    await page.waitForTimeout(100);
    const castleValueAfter = (await castleCard.locator('.cond-source-value').innerText()).trim();
    ok('changing potential to 潜在1 updates the resolved value to +10%', castleValueAfter === '+10%', castleValueAfter);
    await castleCard.locator('select[data-field="potential"]').selectOption('5'); // 元(潜在6)に戻す
    await page.waitForTimeout(100);

    // --- 潜在の選択肢は値が変わる境目だけ(エイヤは潜在1-5/潜在6) ---
    const ayaChip = page.locator('.chip[data-buff-id="aya"]');
    const ayaWasOn = (await ayaChip.getAttribute('aria-pressed')) === 'true';
    if (!ayaWasOn) { await ayaChip.click(); await page.waitForTimeout(100); }
    const ayaPotOpts = await page.locator('.cond-source-card[data-buff-id="aya"] select[data-field="potential"] option').allInnerTexts();
    ok('エイヤ potential options are grouped to 潜在1-5/潜在6', JSON.stringify(ayaPotOpts) === JSON.stringify(['潜在1-5', '潜在6']), JSON.stringify(ayaPotOpts));
    if (!ayaWasOn) { await page.locator('.chip[data-buff-id="aya"]').click(); await page.waitForTimeout(100); }

    // --- 前衛アーミヤ: 「全員」に効き、toggle(スキル中は効果2倍)で解決値が変わる(P4で
    //     旧amiya_guard_normal/amiya_guard_skillの2エントリから1エントリ+toggleへ統合) ---
    const amiyaChip = page.locator('.chip[data-buff-id="amiya_guard"]');
    await amiyaChip.click();
    await page.waitForTimeout(100);
    ok('前衛アーミヤ turns on', (await amiyaChip.getAttribute('aria-pressed')) === 'true');
    const amiyaCard = page.locator('.cond-source-card[data-buff-id="amiya_guard"]');
    const amiyaToggle = amiyaCard.locator('input[data-field="toggleOn"]');
    ok('前衛アーミヤ card has a "スキル中" toggle checkbox', (await amiyaToggle.count()) === 1);
    ok('the toggle is unchecked by default', !(await amiyaToggle.isChecked()));
    const amiyaValueNormal = (await amiyaCard.locator('.cond-source-value').innerText()).trim();
    await amiyaToggle.check();
    await page.waitForTimeout(100);
    const amiyaValueSkill = (await amiyaCard.locator('.cond-source-value').innerText()).trim();
    ok('checking the toggle doubles the resolved value', amiyaValueSkill !== amiyaValueNormal, `${amiyaValueNormal} -> ${amiyaValueSkill}`);
    await page.locator('[data-action="edit-row"]').nth(1).click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const condAshAmiya = await page.locator('.row-conditional-status').innerText();
    ok('前衛アーミヤ(全員) applies (✓) to Ash(狙撃)', /✓\s*前衛アーミヤ/.test(condAshAmiya), condAshAmiya);
    await page.click('[data-action="collapse-row"]');
    await amiyaChip.click(); // 以降の「1件ON」前提のシナリオに影響しないようOFFに戻す
    await page.waitForTimeout(100);

    // --- バフN件バッジが折りたたみ行に出る ---
    const badgeTexts = await page.locator('.badge-buffcount').allTextContents();
    ok('collapsed rows show a "バフN" badge for rows with buffs', badgeTexts.some((t) => /^バフ\d+$/.test(t.trim())), badgeTexts);

    // --- リロードでバフ状態が保持される(localStorage) ---
    await page.waitForTimeout(500);
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    const summaryAfterReload = await page.locator('#global-buffs-details summary').innerText();
    ok('global buff ON state survives reload (localStorage)', summaryAfterReload.includes('1件ON'), summaryAfterReload);
    const badgesAfterReload = await page.locator('.badge-buffcount').allTextContents();
    ok('row buff-count badges survive reload', badgesAfterReload.some((t) => /^バフ\d+$/.test(t.trim())), badgesAfterReload);
    const exusiaiCardAfterReload = page.locator('#individual-buff-levels-section .cond-source-card[data-buff-id="exusiai"]');
    ok('individual buff growth settings section persists after reload (エクシア card still shown)', (await exusiaiCardAfterReload.count()) === 1);
    ok('エクシア resolved value survives reload (+10%, module X restored)',
      (await exusiaiCardAfterReload.locator('.cond-source-value').innerText()).trim() === '+10%',
      await exusiaiCardAfterReload.locator('.cond-source-value').innerText());

    // --- 共有URLにもバフ状態が含まれる ---
    await page.click('[data-action="share"]');
    await page.waitForTimeout(150);
    const sharedUrl = await page.evaluate(() => navigator.clipboard.readText());
    const otherContext = await browser.newContext();
    try {
      const other = await otherContext.newPage();
      await other.setViewportSize({ width: 420, height: 900 });
      await other.goto(sharedUrl, { waitUntil: 'networkidle' });
      await other.waitForTimeout(300);
      const otherSummary = await other.locator('#global-buffs-details summary').innerText();
      ok('shared URL restores the global buff ON state', otherSummary.includes('1件ON'), otherSummary);
      const otherBadges = await other.locator('.badge-buffcount').allTextContents();
      ok('shared URL restores row buff-count badges', otherBadges.some((t) => /^バフ\d+$/.test(t.trim())), otherBadges);
      const otherExusiaiCard = other.locator('#individual-buff-levels-section .cond-source-card[data-buff-id="exusiai"]');
      ok('shared URL restores the individual buff growth settings (エクシア card present)', (await otherExusiaiCard.count()) === 1);
    } finally {
      await otherContext.close();
    }
  } catch (e) {
    fail++;
    console.log(`FAIL  buff scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_buff.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P3: 鼓舞(インスパイア)ソース(濁心スカジ)の表現層テスト。ONにすると鼓舞量が表示され
// 判定が変わること、スキル切替、モジュール条件によるチェックボックス/ヒントの出し分け、
// 行の鼓舞トグル、折りたたみバッジ、手入力バフ+%欄のキー入力、single_target⚠が
// 行と鼓舞ソースを跨いで検出されること、リロード/共有URLでの永続化を検証する。
async function runInspireScenario(browser, baseUrl) {
  const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    // --- Ashを追加してS3/400%、敵HPを設定(鼓舞ONで撃破に変わる値にする) ---
    await addOperator(page, 'Ash', '400%');
    await page.click('[data-action="collapse-row"]');
    await page.fill('#enemy-hp', '7000');
    await page.fill('#enemy-def', '0');
    await page.waitForTimeout(150);
    const verdictBeforeInspire = await page.locator('#verdict-text').innerText();
    ok('verdict before enabling inspire is "足りない" (7000 > Ash単体の6910)', verdictBeforeInspire.includes('足りない'), verdictBeforeInspire);

    // --- ②全体バフを開き、鼓舞ソース(濁心スカジ)のトグルチップをON ---
    await page.click('#global-buffs-details summary');
    await page.waitForTimeout(50);
    const skadiChip = page.locator('.chip[data-source-id="skadi2"]');
    ok('inspire source toggle chip (濁心スカジ) exists', (await skadiChip.count()) === 1);
    await skadiChip.click();
    await page.waitForTimeout(100);
    ok('inspire source card expands after enabling (skill select appears)',
      (await page.locator('select[data-source-id="skadi2"][data-field="skillNum"]').count()) === 1);

    // --- 鼓舞量が表示され、判定が変わる ---
    const resultLineOn = await page.locator('.inspire-result').innerText();
    ok('inspire result line shows a positive amount', /→\s*鼓舞\s*\+\d/.test(resultLineOn), resultLineOn);
    await page.waitForTimeout(150);
    const verdictAfterInspire = await page.locator('#verdict-text').innerText();
    ok('verdict changes after enabling inspire (still shown, value differs)', verdictAfterInspire !== verdictBeforeInspire, `${verdictBeforeInspire} -> ${verdictAfterInspire}`);

    // --- 折りたたみ行に「鼓舞」バッジが出る ---
    const inspireBadge = page.locator('.badge-inspire');
    ok('collapsed row shows a "鼓舞" badge once inspire applies', (await inspireBadge.count()) >= 1);

    // --- スキル切替(S2/S3)で鼓舞量が変わる ---
    const skillSelect = page.locator('select[data-source-id="skadi2"][data-field="skillNum"]');
    await skillSelect.selectOption('2');
    await page.waitForTimeout(100);
    const amountS2 = await page.locator('.inspire-result').innerText();
    await skillSelect.selectOption('3');
    await page.waitForTimeout(100);
    const amountS3 = await page.locator('.inspire-result').innerText();
    ok('switching skill S2 -> S3 changes the inspire amount', amountS2 !== amountS3, `${amountS2} / ${amountS3}`);

    // --- モジュール条件: X未選択では「範囲に他オペ2名以上」はヒント表示、X選択でチェックボックスに変わる ---
    const moduleSelect = page.locator('select[data-source-id="skadi2"][data-field="moduleId"]');
    const moduleLabels = await moduleSelect.locator('option').allTextContents();
    const xIdx = moduleLabels.findIndex((l) => l.includes('（X）'));
    const yIdx = moduleLabels.findIndex((l) => l.includes('（Y）'));
    ok('module select has both X and Y options for 濁心スカジ', xIdx >= 0 && yIdx >= 0, moduleLabels);
    ok('condition part (module_x_two_ops) shows a hint before X is selected',
      (await page.locator('.inspire-source-card .special-hint').count()) >= 1);
    await moduleSelect.selectOption({ index: xIdx });
    await page.waitForTimeout(100);
    ok('selecting module X reveals the condition checkbox instead of the hint',
      (await page.locator('input[data-source-id="skadi2"][data-field^="part:"]').count()) >= 1);

    // --- P8: 潜在セレクト(旧「攻撃凸」+「素質凸」チェックボックスの統合)。濁心スカジは
    //     ATK潜在(潜在4)と素質凸(潜在5)の2つの境目を持つため潜在1-3/潜在4/潜在5-6の3択 ---
    const potentialSelect = page.locator('select[data-source-id="skadi2"][data-field="potential"]');
    ok('inspire source card has a potential select', (await potentialSelect.count()) === 1);
    const potentialOpts = await potentialSelect.locator('option').allInnerTexts();
    ok('potential options are grouped to 潜在1-3/潜在4/潜在5-6', JSON.stringify(potentialOpts) === JSON.stringify(['潜在1-3', '潜在4', '潜在5-6']), JSON.stringify(potentialOpts));
    ok('potential defaults to 潜在5-6 (rank5=潜在6)', (await potentialSelect.inputValue()) === '5', await potentialSelect.inputValue());
    const amountBeforePotential = await page.locator('.inspire-result').innerText();
    await potentialSelect.selectOption('3'); // 潜在4: ATK潜在は乗るが素質凸(潜在5)は未解放
    await page.waitForTimeout(100);
    const amountAfterPotential = await page.locator('.inspire-result').innerText();
    ok('changing potential to 潜在4 changes the inspire amount (talent bonus drops)',
      amountAfterPotential !== amountBeforePotential, `${amountBeforePotential} -> ${amountAfterPotential}`);
    await potentialSelect.selectOption('5'); // 元に戻す
    await page.waitForTimeout(100);

    // --- 行の鼓舞トグル: OFFにするとその行だけ鼓舞が外れる ---
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const rowInspireCheckbox = page.locator('input[data-field="inspireOn"]');
    ok('row inspire toggle checkbox is present and checked by default', await rowInspireCheckbox.isChecked());
    const formulaWithInspire = await page.locator('.row-formula').innerText();
    ok('row formula line includes 鼓舞 while row inspire toggle is ON', formulaWithInspire.includes('鼓舞'), formulaWithInspire);
    // 鼓舞は倍率の前に足すので「(ATK×(1+…) + 鼓舞N) × 倍率」と外側を括弧で囲む
    ok('row formula wraps "ATK×(…) + 鼓舞" in parentheses before the multiplier',
      /^\(\d[\d,]* ×\(1 \+ .*\) \+ 鼓舞[\d,]+\) × /.test(formulaWithInspire), formulaWithInspire);
    await rowInspireCheckbox.uncheck();
    await page.waitForTimeout(100);
    const formulaWithoutInspire = await page.locator('.row-formula').innerText();
    ok('row formula line drops 鼓舞 once the row toggle is OFF', !formulaWithoutInspire.includes('鼓舞'), formulaWithoutInspire);
    await rowInspireCheckbox.check();
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');

    // --- 手入力バフ+%欄: 1打鍵ずつ打っても桁順が保たれる(renderLive経由)。
    //     ②全体バフは既にON操作で開いたままのはず(globalBuffsOpenはrender()を跨いで保持される)
    //     なので、閉じている場合だけ開く(summaryクリックはトグルなので誤って閉じないようにする)。 ---
    const globalBuffsDetails = page.locator('#global-buffs-details');
    if (!(await globalBuffsDetails.evaluate((el) => el.open))) {
      await page.click('#global-buffs-details summary');
      await page.waitForTimeout(50);
    }
    const sourceBuffPctInput = page.locator('input[data-source-id="skadi2"][data-field="buffPct"]');
    await sourceBuffPctInput.fill('');
    await sourceBuffPctInput.pressSequentially('10');
    ok('typing "10" key by key into the source manual buff field yields 10', (await sourceBuffPctInput.inputValue()) === '10',
      await sourceBuffPctInput.inputValue());
    const resultAfterTyping = await page.locator('.inspire-result').innerText();
    ok('inspire result updates live while typing the manual buff field', /→\s*鼓舞\s*\+\d/.test(resultAfterTyping), resultAfterTyping);
    await sourceBuffPctInput.fill('0');
    await page.waitForTimeout(100);

    // --- ⓘ内訳の展開 ---
    const infoBtn = page.locator('.inspire-result-wrap .special-info-btn');
    ok('ⓘ breakdown button is present on the inspire result line', (await infoBtn.count()) === 1);
    await infoBtn.click();
    await page.waitForTimeout(100);
    const breakdownText = await page.locator('.inspire-result-wrap .special-desc').innerText();
    ok('breakdown text mentions the skill ratio formula', breakdownText.includes('×') && breakdownText.includes('='), breakdownText);

    // --- 個別バフ(血漿)を鼓舞ソースへ追加すると量が増える ---
    const beforePlasma = await page.locator('.inspire-result').innerText();
    const plasmaChip = page.locator('.inspire-source-card .chip[data-buff-id="plasma"]');
    ok('individual buff chip (血漿) exists on the inspire source card', (await plasmaChip.count()) === 1);
    await plasmaChip.click();
    await page.waitForTimeout(100);
    const afterPlasma = await page.locator('.inspire-result').innerText();
    ok('adding 血漿(individual buff) to the inspire source raises the amount', afterPlasma !== beforePlasma, `${beforePlasma} -> ${afterPlasma}`);
    await plasmaChip.click(); // 元に戻す(後続シナリオへの影響を避ける)
    await page.waitForTimeout(100);

    // --- single_target(アS3/durian)⚠: 鼓舞ソースと行の両方で選ぶと両方に⚠が出る
    //     (durianはmax_targets_by_moduleを持たないので上限は常に1のまま) ---
    const sourceDurianChip = page.locator('.inspire-source-card .chip[data-buff-id="durian"]');
    await sourceDurianChip.click();
    await page.waitForTimeout(100);
    ok('no ⚠ yet (durian selected only on the inspire source)', (await sourceDurianChip.locator('.chip-warn').count()) === 0);
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const rowDurianChip = page.locator('.row-expanded .chip[data-buff-id="durian"]');
    await rowDurianChip.click();
    await page.waitForTimeout(100);
    ok('⚠ appears on the row once the same single_target buff is also selected on the inspire source',
      (await page.locator('.row-expanded .chip-warn').count()) === 1);
    ok('⚠ also appears on the inspire source chip', (await sourceDurianChip.locator('.chip-warn').count()) === 1);
    await rowDurianChip.click(); // 元に戻す
    await page.waitForTimeout(100);
    await sourceDurianChip.click();
    await page.waitForTimeout(100);

    // --- エクシア(max_targets_by_module=2。P5): 育成設定が既定(モジュールX Lv2以上)の間は
    //     ソース+行の合計2箇所選んでも⚠が出ない ---
    const sourceExusiaiChip = page.locator('.inspire-source-card .chip[data-buff-id="exusiai"]');
    await sourceExusiaiChip.click();
    await page.waitForTimeout(100);
    const rowExusiaiChip = page.locator('.row-expanded .chip[data-buff-id="exusiai"]');
    await rowExusiaiChip.click();
    await page.waitForTimeout(100);
    ok('no ⚠ for エクシア with source+row=2 selections (max_targets_by_module=2 by default)',
      (await page.locator('.row-expanded .chip-warn').count()) === 0 && (await sourceExusiaiChip.locator('.chip-warn').count()) === 0);
    await rowExusiaiChip.click(); // 元に戻す
    await page.waitForTimeout(100);
    await sourceExusiaiChip.click();
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');

    // --- リロードで鼓舞の設定が保持される(localStorage) ---
    await page.waitForTimeout(500);
    const summaryBeforeReload = await page.locator('#global-buffs-details summary').innerText();
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    const summaryAfterReload = await page.locator('#global-buffs-details summary').innerText();
    ok('inspire ON state survives reload (localStorage)', summaryAfterReload === summaryBeforeReload, `${summaryBeforeReload} / ${summaryAfterReload}`);
    ok('inspire source card is still expanded after reload',
      (await page.locator('select[data-source-id="skadi2"][data-field="skillNum"]').count()) === 1);

    // --- 共有URLにも鼓舞の設定が含まれる ---
    await page.click('[data-action="share"]');
    await page.waitForTimeout(150);
    const sharedUrl = await page.evaluate(() => navigator.clipboard.readText());
    const otherContext = await browser.newContext();
    try {
      const other = await otherContext.newPage();
      await other.setViewportSize({ width: 420, height: 900 });
      await other.goto(sharedUrl, { waitUntil: 'networkidle' });
      await other.waitForTimeout(300);
      const otherSummary = await other.locator('#global-buffs-details summary').innerText();
      ok('shared URL restores the inspire ON state', otherSummary.includes('鼓舞'), otherSummary);
      ok('shared URL restores the expanded inspire source card',
        (await other.locator('select[data-source-id="skadi2"][data-field="skillNum"]').count()) === 1);
    } finally {
      await otherContext.close();
    }
  } catch (e) {
    fail++;
    console.log(`FAIL  inspire scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_inspire.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P4: 条件付きバフの動的値解決(昇進/潜在/モジュール/スキルLv)のインライン選択UIの
// 表現層テスト。スズラン(モジュール限定。未装備時はヒント)・ズィマー(スキルLv選択)・
// 選択のlocalStorage永続化を検証する。
async function runConditionalSourceScenario(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });
    await page.click('#global-buffs-details summary');
    await page.waitForTimeout(50);

    // --- スズラン: デフォルト(最大成長)はモジュールX Lv3が最初から選ばれた状態で+9%。
    //     モジュールを「なし」に戻すとヒント表示(+0%、base has no atk key)に切り替わる ---
    const suzuranChip = page.locator('.chip[data-buff-id="suzuran"]');
    ok('suzuran chip exists', (await suzuranChip.count()) === 1);
    await suzuranChip.click();
    await page.waitForTimeout(100);
    const suzuranCard = page.locator('.cond-source-card[data-buff-id="suzuran"]');
    ok('suzuran defaults to module X selected already (max-growth default) with a level select',
      (await suzuranCard.locator('select[data-field="moduleLevel"]').count()) === 1 && (await suzuranCard.locator('.special-hint').count()) === 0);
    const suzuranValueDefault = (await suzuranCard.locator('.cond-source-value').innerText()).trim();
    ok('suzuran default resolved value is +9% (module X Lv3)', suzuranValueDefault === '+9%', suzuranValueDefault);
    const suzuranModuleSelect = suzuranCard.locator('select[data-field="moduleId"]');
    const suzuranModuleLabels = await suzuranModuleSelect.locator('option').allTextContents();
    ok('suzuran module select offers なし and X', suzuranModuleLabels.some((l) => l.trim() === 'X') && suzuranModuleLabels.some((l) => l.trim() === 'なし'), suzuranModuleLabels);
    await suzuranModuleSelect.selectOption({ label: 'なし' });
    await page.waitForTimeout(100);
    ok('switching module to なし shows the hint instead of a level select (base has no atk key)',
      (await suzuranCard.locator('.special-hint').count()) === 1 && (await suzuranCard.locator('select[data-field="moduleLevel"]').count()) === 0);
    const suzuranValueNoModule = (await suzuranCard.locator('.cond-source-value').innerText()).trim();
    ok('suzuran resolved value is +0% without a module', suzuranValueNoModule === '+0%', suzuranValueNoModule);
    await suzuranModuleSelect.selectOption({ label: 'X' });
    await page.waitForTimeout(100);
    ok('selecting module X again reveals the level select and hides the hint',
      (await suzuranCard.locator('select[data-field="moduleLevel"]').count()) === 1 && (await suzuranCard.locator('.special-hint').count()) === 0);
    const suzuranValueAfter = (await suzuranCard.locator('.cond-source-value').innerText()).trim();
    ok('suzuran resolved value is +9% again with module X at its default level (Lv3)', suzuranValueAfter === '+9%', suzuranValueAfter);
    await suzuranCard.locator('select[data-field="moduleLevel"]').selectOption('2');
    await page.waitForTimeout(100);
    const suzuranValueLv2 = (await suzuranCard.locator('.cond-source-value').innerText()).trim();
    ok('switching module X to Lv2 changes the resolved value to +6%', suzuranValueLv2 === '+6%', suzuranValueLv2);

    // --- ズィマー: スキルLv別blackboard由来。Lvセレクトを変えると解決値が変わる ---
    const zimaChip = page.locator('.chip[data-buff-id="zima"]');
    await zimaChip.click();
    await page.waitForTimeout(100);
    const zimaCard = page.locator('.cond-source-card[data-buff-id="zima"]');
    const zimaSkillSelect = zimaCard.locator('select[data-field="skillLevel"]');
    ok('zima card has a skill level select', (await zimaSkillSelect.count()) === 1);
    const zimaLabels = await zimaSkillSelect.locator('option').allTextContents();
    ok('zima skill level select has 10 levels labelled SLv1..SLv7 + 特化1..3',
      JSON.stringify(zimaLabels) === JSON.stringify(['SLv1', 'SLv2', 'SLv3', 'SLv4', 'SLv5', 'SLv6', 'SLv7', '特化1', '特化2', '特化3']), zimaLabels);
    ok('zima chip is labelled ズィマーS2', (await zimaChip.innerText()).includes('ズィマーS2'), await zimaChip.innerText());
    const zimaValueDefault = (await zimaCard.locator('.cond-source-value').innerText()).trim();
    ok('zima default (特化3) resolves to +60%', zimaValueDefault === '+60%', zimaValueDefault);
    await zimaSkillSelect.selectOption('1');
    await page.waitForTimeout(100);
    const zimaValueLv1 = (await zimaCard.locator('.cond-source-value').innerText()).trim();
    ok('switching zima to skill level 1 resolves to +25%', zimaValueLv1 === '+25%', zimaValueLv1);

    // --- リロードで選択(モジュールLv2・スキルLv1)が保持される(localStorage) ---
    await page.waitForTimeout(500);
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    if (!(await page.locator('#global-buffs-details').evaluate((el) => el.open))) {
      await page.click('#global-buffs-details summary');
      await page.waitForTimeout(50);
    }
    const suzuranValueReloaded = (await page.locator('.cond-source-card[data-buff-id="suzuran"] .cond-source-value').innerText()).trim();
    ok('suzuran module Lv2 selection survives reload (localStorage)', suzuranValueReloaded === '+6%', suzuranValueReloaded);
    const zimaValueReloaded = (await page.locator('.cond-source-card[data-buff-id="zima"] .cond-source-value').innerText()).trim();
    ok('zima skill level 1 selection survives reload (localStorage)', zimaValueReloaded === '+25%', zimaValueReloaded);
  } catch (e) {
    fail++;
    console.log(`FAIL  conditional-source scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_conditional_source.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P5: 個別バフの動的値解決(talent×scale/base_pct×scale/stage)と「個別バフの育成設定」
// セクションの表現層テスト。何も選んでいない間はヒントのみ、行でバフを選ぶとカードが
// 現れること・軸の表示/非表示(scaleのvaries)・段階セレクト・トグル(装置2台)・
// リロードでの永続化を検証する。
async function runIndividualBuffLevelsScenario(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    // --- 何も選んでいない間はヒントのみ(カード無し) ---
    const section = page.locator('#individual-buff-levels-section');
    ok('individual buff growth settings section shows only a hint when nothing is checked',
      (await section.locator('.cond-source-card').count()) === 0 && (await section.innerText()).includes('育成状況'));

    // --- ステインレスS1をチェックするとカードが現れ、トグル(装置2台)で解決値が変わる ---
    await addOperator(page, 'Ash', '400%');
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    await page.locator('.row-expanded .chip[data-buff-id="stainless_s1"]').click();
    await page.waitForTimeout(100);
    const stainlessCard = section.locator('.cond-source-card[data-buff-id="stainless_s1"]');
    ok('ステインレスS1 card appears once checked on a row', (await stainlessCard.count()) === 1);
    const stainlessValueBefore = (await stainlessCard.locator('.cond-source-value').innerText()).trim();
    ok('ステインレスS1 default resolved value is +48% (base_pct 0.12 × scale Lv10=4)', stainlessValueBefore === '+48%', stainlessValueBefore);
    const stainlessToggle = stainlessCard.locator('input[data-field="toggleOn"]');
    ok('ステインレスS1 card has a "装置2台" toggle checkbox, unchecked by default', (await stainlessToggle.count()) === 1 && !(await stainlessToggle.isChecked()));
    await stainlessToggle.check();
    await page.waitForTimeout(100);
    const stainlessValueAfter = (await stainlessCard.locator('.cond-source-value').innerText()).trim();
    ok('checking "装置2台" doubles the resolved value to +96%', stainlessValueAfter === '+96%', stainlessValueAfter);
    // 行のチップ表示も連動して更新される。
    const stainlessChipText = await page.locator('.row-expanded .chip[data-buff-id="stainless_s1"]').innerText();
    ok('the row chip label also reflects the toggled value (+96%)', stainlessChipText.includes('96%'), stainlessChipText);

    // --- 育成設定を変えてもスクロール位置が動かない(render後のフォーカス復元が同じ
    //     data-fieldの別カードへ飛んでスクロールしていた不具合の回帰テスト) ---
    await stainlessCard.scrollIntoViewIfNeeded();
    const scrollResult = await page.evaluate(async () => {
      const el = document.querySelector('#individual-buff-levels-section .cond-source-card[data-buff-id="stainless_s1"] input[data-field="toggleOn"]');
      const before = window.scrollY;
      el.focus({ preventScroll: true });
      el.click();
      await new Promise((r) => setTimeout(r, 200));
      el.ownerDocument.querySelector('#individual-buff-levels-section .cond-source-card[data-buff-id="stainless_s1"] input[data-field="toggleOn"]').click();
      await new Promise((r) => setTimeout(r, 200));
      return { before, after: window.scrollY, activeBuff: document.activeElement && document.activeElement.dataset.buffId };
    });
    ok('toggling a growth setting keeps the scroll position and focus on the same card',
      scrollResult.before === scrollResult.after && scrollResult.activeBuff === 'stainless_s1', JSON.stringify(scrollResult));
    ok('changing an individual growth setting does not expand the global buff panel',
      !(await page.locator('#global-buffs-details').evaluate((el) => el.open)));

    // --- スワイヤーS1(scale固定値なのでスキルLv軸は非表示)/S2(scaleが変化するので表示) ---
    await page.locator('.row-expanded .chip[data-buff-id="swire_s1"]').click();
    await page.locator('.row-expanded .chip[data-buff-id="swire_s2"]').click();
    await page.waitForTimeout(100);
    const swireS1Card = section.locator('.cond-source-card[data-buff-id="swire_s1"]');
    const swireS2Card = section.locator('.cond-source-card[data-buff-id="swire_s2"]');
    ok('swire_s1 card has elite/potential selects (talent varies) but no skill level select (scale is constant 2.0)',
      (await swireS1Card.locator('select[data-field="elite"]').count()) === 1 &&
        (await swireS1Card.locator('select[data-field="potential"]').count()) === 1 &&
        (await swireS1Card.locator('select[data-field="skillLevel"]').count()) === 0);
    ok('swire_s2 card DOES have a skill level select (scale varies 2.1〜3.0)',
      (await swireS2Card.locator('select[data-field="skillLevel"]').count()) === 1);
    const swireS2ValueBefore = (await swireS2Card.locator('.cond-source-value').innerText()).trim();
    ok('swire_s2 default resolved value is +36%', swireS2ValueBefore === '+36%', swireS2ValueBefore);
    await swireS2Card.locator('select[data-field="skillLevel"]').selectOption('1');
    await page.waitForTimeout(100);
    const swireS2ValueLv1 = (await swireS2Card.locator('.cond-source-value').innerText()).trim();
    ok('switching swire_s2 to skill level 1 changes the resolved value to +25% (25.2 rounded)', swireS2ValueLv1 === '+25%', swireS2ValueLv1);

    // --- ナスティS3: 段階セレクト(1段階/2段階/3段階) ---
    await page.locator('.row-expanded .chip[data-buff-id="nasty_s3"]').click();
    await page.waitForTimeout(100);
    const nastyCard = section.locator('.cond-source-card[data-buff-id="nasty_s3"]');
    const nastyStageSelect = nastyCard.locator('select[data-field="stageIndex"]');
    ok('nasty_s3 card has a stage select', (await nastyStageSelect.count()) === 1);
    const nastyStageLabels = await nastyStageSelect.locator('option').allTextContents();
    ok('nasty_s3 stage select offers 1段階/2段階/3段階', JSON.stringify(nastyStageLabels) === JSON.stringify(['1段階', '2段階', '3段階']), nastyStageLabels);
    const nastyValueBefore = (await nastyCard.locator('.cond-source-value').innerText()).trim();
    ok('nasty_s3 default (3段階) resolves to +60%', nastyValueBefore === '+60%', nastyValueBefore);
    await nastyStageSelect.selectOption('1');
    await page.waitForTimeout(100);
    const nastyValueLv1 = (await nastyCard.locator('.cond-source-value').innerText()).trim();
    ok('switching nasty_s3 to 1段階 resolves to +20%', nastyValueLv1 === '+20%', nastyValueLv1);

    // --- リロードで育成設定(トグル/スキルLv/段階)が保持される(localStorage) ---
    await page.waitForTimeout(500);
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    const stainlessValueReloaded = (await page.locator('#individual-buff-levels-section .cond-source-card[data-buff-id="stainless_s1"] .cond-source-value').innerText()).trim();
    ok('stainless_s1 toggle survives reload (+96%)', stainlessValueReloaded === '+96%', stainlessValueReloaded);
    const swireS2ValueReloaded = (await page.locator('#individual-buff-levels-section .cond-source-card[data-buff-id="swire_s2"] .cond-source-value').innerText()).trim();
    ok('swire_s2 skill level selection survives reload (+25%)', swireS2ValueReloaded === '+25%', swireS2ValueReloaded);
    const nastyValueReloaded = (await page.locator('#individual-buff-levels-section .cond-source-card[data-buff-id="nasty_s3"] .cond-source-value').innerText()).trim();
    ok('nasty_s3 stage selection survives reload (+20%)', nastyValueReloaded === '+20%', nastyValueReloaded);
  } catch (e) {
    fail++;
    console.log(`FAIL  individual-buff-levels scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_individual_buff_levels.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P1形(buffIds/specialOn/globalBuffIds無し)のlocalStorage stateがそのまま読み込めること。
async function runOldShapeLocalStorageScenario(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.evaluate(() => {
      // P1当時のフィールドのみを持つstate(buffIds/specialOn/globalBuffIds無し)。
      const oldState = {
        v: 1,
        enemy: { hp: 5000, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 },
        rows: [{
          opId: 'char_456_ash', entryIdx: 0, dmgType: 'physical', potential: true,
          moduleId: null, moduleLv: 3, multiplier: 3, selfPct: 0, hits: 1, buffPct: 0, dmgMult: 1, ignoreDef: 0,
        }],
      };
      localStorage.setItem('fkc-state-v1', JSON.stringify(oldState));
    });
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    ok('old P1-shaped localStorage state loads without error (1 row)', (await page.locator('.row-card').count()) === 1);
    const verdictText = await page.locator('#verdict-text').innerText();
    ok('old P1-shaped localStorage state computes a verdict',
      verdictText.includes('撃破できる') || verdictText.includes('足りない'), verdictText);

    // --- P8: 旧boolean形式のpotential(true)は丁寧な移行をせず既定値(5=潜在6)へ
    //     リセットされる(オーナー指示。詳細はengine.jsのdropStaleRowsコメント参照) ---
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const potentialSelect = page.locator('select[data-field="potential"][data-idx="0"]');
    ok('old boolean potential falls back to the default 潜在6(rank5)', (await potentialSelect.inputValue()) === '5', await potentialSelect.inputValue());
  } catch (e) {
    fail++;
    console.log(`FAIL  old-shape localStorage scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
  } finally {
    await context.close();
  }
}

// P8: 行/鼓舞ソースの潜在セレクト(旧「攻撃凸」/「攻撃凸+素質凸」チェックボックスの後継)。
async function runPotentialScenario(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    // --- ATK潜在を持つオペレーター(Ash。潜在4で+27の1段階のみ)は潜在セレクトが出て、
    //     値が変わる境目だけが選択肢になる(潜在1-3/潜在4-6) ---
    await addOperator(page, 'Ash', '400%');
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const potentialSelect = page.locator('select[data-field="potential"][data-idx="0"]');
    ok('row potential select is present for an operator with ATK potential (Ash)', (await potentialSelect.count()) === 1);
    const potentialOpts = await potentialSelect.locator('option').allInnerTexts();
    ok('Ash potential options are grouped to 潜在1-3/潜在4-6', JSON.stringify(potentialOpts) === JSON.stringify(['潜在1-3', '潜在4-6']), JSON.stringify(potentialOpts));
    ok('Ash potential defaults to 潜在4-6 (rank5=潜在6)', (await potentialSelect.inputValue()) === '5', await potentialSelect.inputValue());

    const formulaBefore = await page.locator('.row-formula').innerText();
    await potentialSelect.selectOption('2'); // 潜在1-3(ATK潜在が乗らない)
    await page.waitForTimeout(100);
    const formulaAfter = await page.locator('.row-formula').innerText();
    ok('changing the potential select updates the row damage total', formulaAfter !== formulaBefore, `${formulaBefore} -> ${formulaAfter}`);
    await potentialSelect.selectOption('5'); // 元に戻す
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');

    // --- ATK潜在を持たないオペレーター(Fuze)は潜在セレクト自体が出ない(隠す仕様) ---
    await addOperator(page, 'Fuze', null);
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    const noPotentialSelect = page.locator('select[data-field="potential"][data-idx="1"]');
    ok('row potential select is hidden for an operator without ATK potential (Fuze)', (await noPotentialSelect.count()) === 0);
    await page.click('[data-action="collapse-row"]');

    // --- リロード/共有URLでも選択が保持される ---
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    await page.locator('select[data-field="potential"][data-idx="0"]').selectOption('2');
    await page.waitForTimeout(500);
    await page.click('[data-action="collapse-row"]');
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    ok('potential selection survives reload (localStorage)',
      (await page.locator('select[data-field="potential"][data-idx="0"]').inputValue()) === '2');
  } catch (e) {
    fail++;
    console.log(`FAIL  potential scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_potential.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P8 follow-up: 特殊強化(乗算系mulMultiplier)が固定値ではなく行自身の昇進/潜在に
// 追従すること(ファイヤーウォッチS2「遠距離特効」)。verify.mjsの計算結果(潜在4は×1.5、
// E2/潜在6/モジュールYLv3は×1.55、E0は素質未解放で×1[ヒント])を表現層で確認する。
async function runFwSpecialScenario(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    await addOperator(page, 'ファイヤーウォッチ', null);
    await page.waitForSelector('.row-expanded', { timeout: 5000 });

    // --- 既定(E2・潜在6・モジュールYLv3)ではチェックボックスが出て、ⓘ現在値は×1.55 ---
    const specialCheckbox = page.locator('[data-field="specialOn"]');
    ok('special checkbox (遠距離特効) is present at E2/潜在6', (await specialCheckbox.count()) === 1);
    const infoBtn = page.locator('.special-info-btn');
    await infoBtn.click();
    await page.waitForTimeout(100);
    const descAtDefault = await page.locator('.special-desc').innerText();
    ok('special description shows the default current value (現在: ×1.55)', descAtDefault.includes('現在: ×1.55'), descAtDefault);
    await infoBtn.click();
    await page.waitForTimeout(100);

    // --- 潜在セレクトの選択肢は「潜在1-3/潜在4/潜在5-6」の3択(ATK潜在境目+素質境目の和) ---
    const potentialSelect = page.locator('select[data-field="potential"][data-idx="0"]');
    const potentialOpts = await potentialSelect.locator('option').allInnerTexts();
    ok(
      'FW potential options are grouped to 潜在1-3/潜在4/潜在5-6',
      JSON.stringify(potentialOpts) === JSON.stringify(['潜在1-3', '潜在4', '潜在5-6']),
      JSON.stringify(potentialOpts),
    );

    // --- 潜在4へ変えるとⓘ現在値が×1.5になり、合計ダメージも変わる ---
    const formulaBefore = await page.locator('.row-formula').innerText();
    await potentialSelect.selectOption('3'); // 潜在4
    await page.waitForTimeout(100);
    await page.locator('.special-info-btn').click();
    await page.waitForTimeout(100);
    const descAtPot4 = await page.locator('.special-desc').innerText();
    ok(
      'changing potential to 潜在4 changes the ⓘ current value to ×1.5',
      descAtPot4.includes('現在: ×1.5') && !descAtPot4.includes('現在: ×1.55'),
      descAtPot4,
    );
    const formulaAfter = await page.locator('.row-formula').innerText();
    ok('changing potential to 潜在4 changes the row total', formulaAfter !== formulaBefore, `${formulaBefore} -> ${formulaAfter}`);
    await page.locator('.special-info-btn').click();
    await page.waitForTimeout(100);
    await potentialSelect.selectOption('5'); // 元(潜在6)へ戻す
    await page.waitForTimeout(100);

    // --- 昇進をE0まで下げると素質未解放になり、チェックボックスの代わりにヒントが出る ---
    const eliteSelect = page.locator('select[data-field="elite"][data-idx="0"]');
    await eliteSelect.selectOption('0');
    await page.waitForTimeout(100);
    ok('at E0 the special checkbox disappears (talent not unlocked)', (await page.locator('[data-field="specialOn"]').count()) === 0);
    // E0では他の警告(スキル解放/モジュール装備可否)も.special-hintを共有するため、
    // 複数件から目的のテキストを含むものを探す(elite/level/trustシナリオと同じ手法)。
    const hintTexts = await page.locator('.row-expanded .special-hint').allInnerTexts();
    ok('E0 shows the talent-unlock hint (素質が昇進1で解放)', hintTexts.some((t) => t.includes('素質が昇進1で解放')), hintTexts);
    await eliteSelect.selectOption('2');
    await page.waitForTimeout(100);
    ok('switching back to E2 restores the checkbox', (await page.locator('[data-field="specialOn"]').count()) === 1);
  } catch (e) {
    fail++;
    console.log(`FAIL  FW special scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_fw_special.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P6: 昇進(elite)/レベル(level)/信頼度(trust)コントロール。
async function runEliteLevelTrustScenario(browser, baseUrl) {
  const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await context.newPage();
  try {
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    await addOperator(page, 'Ash', '400%');
    const eliteSelect = page.locator('.row-expanded select[data-field="elite"]');
    const levelInput = page.locator('.row-expanded input[data-field="level"]');
    const trustInput = page.locator('.row-expanded input[data-field="trust"]');

    // --- 既定値: E2・そのオペレーターの最大レベル(90)・信頼度100% ---
    ok('elite/level/trust controls appear on the expanded row',
      (await eliteSelect.count()) === 1 && (await levelInput.count()) === 1 && (await trustInput.count()) === 1);
    ok('default elite is E2', (await eliteSelect.inputValue()) === '2', await eliteSelect.inputValue());
    ok('default level is the operator max (90)', (await levelInput.inputValue()) === '90', await levelInput.inputValue());
    ok('default trust is 100', (await trustInput.inputValue()) === '100', await trustInput.inputValue());

    // --- 昇進を変えるとレベルがその昇進の最大値へリセットされ、スキル解放警告が出る ---
    await eliteSelect.selectOption('1');
    await page.waitForTimeout(100);
    ok('changing elite to E1 resets level to that phase max (80)', (await levelInput.inputValue()) === '80', await levelInput.inputValue());
    const warningText = (await page.locator('.row-expanded .special-hint').first().innerText()).trim();
    ok('skill unlock warning shown for S3 while at E1 (S3 unlocks at E2)', warningText.includes('S3は昇進2で解放'), warningText);

    await eliteSelect.selectOption('2');
    await page.waitForTimeout(100);
    ok('level resets back to the E2 max (90) after returning to E2', (await levelInput.inputValue()) === '90');
    ok('skill unlock warning disappears once back at E2',
      !(await page.locator('.row-expanded .special-hint').allInnerTexts()).some((t) => t.includes('は昇進')));

    // --- 実キー入力でレベルを打てる(1打鍵ずつでも桁順が保たれる) ---
    await levelInput.fill('');
    await levelInput.pressSequentially('60');
    ok('typing "60" key by key into level yields 60', (await levelInput.inputValue()) === '60', await levelInput.inputValue());
    await page.waitForTimeout(150);

    // --- モジュール装備可否のヒント: Ashのモジュールは昇進2 Lv60以上で装備可能 ---
    // (デフォルトのLv90では既に装備可能なので、Lv59まで下げて初めてヒントが出ることを確認する)
    await levelInput.fill('');
    await levelInput.pressSequentially('59');
    await page.waitForTimeout(150);
    const hintAt59 = (await page.locator('.module-hint-wrap').first().innerText()).trim();
    ok('module-unusable hint appears once level drops below the unlock level (59 < 60)',
      hintAt59.includes('昇進2 Lv60以上で装備可能'), hintAt59);
    const formulaAt59 = await page.locator('.row-formula').innerText();
    const atkAt59 = Number(formulaAt59.match(/^([\d,]+)/)[1].replace(/,/g, ''));

    await levelInput.fill('');
    await levelInput.pressSequentially('60');
    await page.waitForTimeout(150);
    ok('module-unusable hint disappears once level reaches the unlock level (60)',
      (await page.locator('.module-hint-wrap').first().innerText()).trim() === '');
    const formulaAt60 = await page.locator('.row-formula').innerText();
    const atkAt60 = Number(formulaAt60.match(/^([\d,]+)/)[1].replace(/,/g, ''));
    ok('ATK increases once the module becomes usable again', atkAt60 > atkAt59, `${atkAt59} -> ${atkAt60}`);

    // --- 信頼度を下げると合計(ATK)が下がる ---
    await trustInput.fill('');
    await trustInput.pressSequentially('50');
    await page.waitForTimeout(150);
    const formulaTrust50 = await page.locator('.row-formula').innerText();
    const atkTrust50 = Number(formulaTrust50.match(/^([\d,]+)/)[1].replace(/,/g, ''));
    ok('lowering trust decreases ATK (and the total)', atkTrust50 < atkAt60, `${atkAt60} -> ${atkTrust50}`);
    await trustInput.fill('');
    await trustInput.pressSequentially('100');
    await page.waitForTimeout(150);

    // --- 折りたたみ時のサマリーラベル ("E2 Lv60"。信頼度100%は省略) ---
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);
    const eliteBadgeText = (await page.locator('.badge-elite').first().innerText()).trim();
    ok('collapsed row summary shows "E2 Lv60" (trust 100% omitted)', eliteBadgeText === 'E2 Lv60', eliteBadgeText);

    // --- 信頼度が100%でない時はラベルに付け足される ---
    await page.locator('[data-action="edit-row"]').first().click();
    await page.waitForTimeout(100);
    await page.locator('.row-expanded input[data-field="trust"]').fill('');
    await page.locator('.row-expanded input[data-field="trust"]').pressSequentially('60');
    await page.waitForTimeout(150);
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);
    const eliteBadgeTrust = (await page.locator('.badge-elite').first().innerText()).trim();
    ok('collapsed row summary appends trust when it is not 100%', eliteBadgeTrust === 'E2 Lv60 信頼60%', eliteBadgeTrust);

    // --- リロードしても昇進/レベル/信頼度が保持される(localStorage) ---
    await page.waitForTimeout(500);
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    const eliteBadgeReloaded = (await page.locator('.badge-elite').first().innerText()).trim();
    ok('elite/level/trust survive reload', eliteBadgeReloaded === 'E2 Lv60 信頼60%', eliteBadgeReloaded);

    // --- 共有URLでも同じ状態が復元される ---
    await page.click('[data-action="share"]');
    await page.waitForTimeout(100);
    const shareUrl = await page.evaluate(() => navigator.clipboard.readText());
    const page2 = await context.newPage();
    await page2.goto(shareUrl, { waitUntil: 'networkidle' });
    await page2.waitForTimeout(300);
    const eliteBadgeShared = (await page2.locator('.badge-elite').first().innerText()).trim();
    ok('shared URL restores elite/level/trust', eliteBadgeShared === 'E2 Lv60 信頼60%', eliteBadgeShared);
    await page2.close();

    // --- 鼓舞ソース(濁心スカジ)にも同じ昇進/レベル/信頼度コントロールがある ---
    await page.locator('#global-buffs-details summary').click();
    await page.waitForTimeout(100);
    await page.locator('.chip[data-action="toggle-inspire-source"][data-source-id="skadi2"]').click();
    await page.waitForTimeout(100);
    const sourceCard = page.locator('.inspire-source-card[data-source-id="skadi2"]');
    ok('inspire source card has its own elite/level/trust controls',
      (await sourceCard.locator('select[data-field="elite"]').count()) === 1 &&
        (await sourceCard.locator('input[data-field="level"]').count()) === 1 &&
        (await sourceCard.locator('input[data-field="trust"]').count()) === 1);
  } catch (e) {
    fail++;
    console.log(`FAIL  elite/level/trust scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_elite_level_trust.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P7: スキルLv(SLv1〜7+特化1〜3)コントロール。
// row-formula の2行目("→ 実ダメ ... = <b>合計</b> ...")から合計ダメージを取り出す。
// テキスト中に"="が複数回出る(1行目のfinal算出にも"="がある)ので、最後の"="の後の数値を使う。
function lastEqualsNumber(text) {
  const matches = [...text.matchAll(/=\s*([\d,]+)/g)];
  return Number(matches[matches.length - 1][1].replace(/,/g, ''));
}

async function runSkillLevelScenario(browser, baseUrl) {
  const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await context.newPage();
  try {
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    await addOperator(page, 'Ash', '400%');
    const skillLevelSelect = page.locator('.row-expanded select[data-field="skillLevel"]');
    const eliteSelect = page.locator('.row-expanded select[data-field="elite"]');

    // --- 既定値: 特化3(10) ---
    ok('skill level select appears on the expanded row', (await skillLevelSelect.count()) === 1);
    ok('default skill level is 特化3 (value 10)', (await skillLevelSelect.inputValue()) === '10', await skillLevelSelect.inputValue());
    const selectedLabel = await skillLevelSelect.locator('option:checked').innerText();
    ok('default skill level label reads 特化3', selectedLabel === '特化3', selectedLabel);

    const formulaAtLv10 = await page.locator('.row-formula').innerText();
    const totalAtLv10 = lastEqualsNumber(formulaAtLv10);

    // --- スキルLvを下げると倍率(400%バリアントのnot_hitwall_scale)が下がり、合計も下がる ---
    await skillLevelSelect.selectOption('1');
    await page.waitForTimeout(100);
    const multiplierInput = page.locator('.row-expanded input[data-field="multiplier"]');
    ok('lowering skill level to SLv1 re-snaps the multiplier input to 3 (400% variant at L1)',
      (await multiplierInput.inputValue()) === '3', await multiplierInput.inputValue());
    const formulaAtLv1 = await page.locator('.row-formula').innerText();
    const totalAtLv1 = lastEqualsNumber(formulaAtLv1);
    ok('total damage decreases when skill level is lowered (lower multiplier)', totalAtLv1 < totalAtLv10, `${totalAtLv10} -> ${totalAtLv1}`);

    // --- FK対象を切り替えてもスキルLvは保たれる(バリアント違い400%→800%。別スキルへの
    //     切り替えは末尾のウァン(S2→S3)で確認する) ---
    await selectEntryByLabel(page, '800%');
    await page.waitForTimeout(100);
    ok('switching to another variant of the same skill keeps the skill level (SLv1)',
      (await skillLevelSelect.inputValue()) === '1', await skillLevelSelect.inputValue());
    ok('the 800% variant at SLv1 re-snaps the multiplier to 6 (hitwall_scale L1)',
      (await multiplierInput.inputValue()) === '6', await multiplierInput.inputValue());
    await selectEntryByLabel(page, '400%');
    await page.waitForTimeout(100);

    // --- 昇進不足で特化を選ぶと警告が出る(計算は続行される) ---
    await eliteSelect.selectOption('0');
    await page.waitForTimeout(100);
    await skillLevelSelect.selectOption('8');
    await page.waitForTimeout(100);
    const warningText = (await page.locator('.row-expanded .special-hint').allInnerTexts()).join(' / ');
    ok('warning shown when 特化1 is selected at E0 (特化 needs E2)', warningText.includes('特化は昇進2で解放'), warningText);

    await eliteSelect.selectOption('2');
    await page.waitForTimeout(100);
    await skillLevelSelect.selectOption('7');
    await page.waitForTimeout(100);

    // --- 折りたたみ時のサマリーに非既定のスキルLvが付け足される ---
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);
    const eliteBadgeText = (await page.locator('.badge-elite').first().innerText()).trim();
    ok('collapsed row summary appends skill level when not 特化3 (e.g. "SLv7")', eliteBadgeText.endsWith('SLv7'), eliteBadgeText);

    // --- リロードしてもスキルLvが保持される ---
    await page.waitForTimeout(500);
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    const eliteBadgeReloaded = (await page.locator('.badge-elite').first().innerText()).trim();
    ok('skill level survives reload', eliteBadgeReloaded.endsWith('SLv7'), eliteBadgeReloaded);

    // --- 共有URLでも同じ状態が復元される ---
    await page.click('[data-action="share"]');
    await page.waitForTimeout(100);
    const shareUrl = await page.evaluate(() => navigator.clipboard.readText());
    const page2 = await context.newPage();
    await page2.goto(shareUrl, { waitUntil: 'networkidle' });
    await page2.waitForTimeout(300);
    const eliteBadgeShared = (await page2.locator('.badge-elite').first().innerText()).trim();
    ok('shared URL restores skill level', eliteBadgeShared.endsWith('SLv7'), eliteBadgeShared);
    await page2.close();

    // --- 鼓舞ソース(濁心スカジ)にもスキルLvセレクトがある ---
    await page.locator('#global-buffs-details summary').click();
    await page.waitForTimeout(100);
    await page.locator('.chip[data-action="toggle-inspire-source"][data-source-id="skadi2"]').click();
    await page.waitForTimeout(100);
    const sourceCard = page.locator('.inspire-source-card[data-source-id="skadi2"]');
    const sourceSkillLevelSelect = sourceCard.locator('select[data-field="skillLevel"]');
    ok('inspire source card has its own skill level select', (await sourceSkillLevelSelect.count()) === 1);
    ok('inspire source default skill level is 特化3 (value 10)', (await sourceSkillLevelSelect.inputValue()) === '10');

    const resultAtLv10 = (await sourceCard.locator('.inspire-result').innerText()).trim();
    await sourceSkillLevelSelect.selectOption('1');
    await page.waitForTimeout(100);
    const resultAtLv1 = (await sourceCard.locator('.inspire-result').innerText()).trim();
    ok('changing the inspire source skill level changes its result', resultAtLv1 !== resultAtLv10, `${resultAtLv10} -> ${resultAtLv1}`);
    // 鼓舞ソースのスキルを切り替えてもスキルLvは保たれる
    await sourceCard.locator('select[data-field="skillNum"]').selectOption('3');
    await page.waitForTimeout(100);
    ok('switching the inspire source skill keeps its skill level (SLv1)',
      (await page.locator('.inspire-source-card[data-source-id="skadi2"] select[data-field="skillLevel"]').inputValue()) === '1');

    // --- 別スキルのFK対象へ切り替えてもスキルLvは保たれる(ウァン S2→S3。オーナー実機報告) ---
    await addOperator(page, 'ウァン', 'S2');
    const wanSkillLevel = page.locator('.row-expanded select[data-field="skillLevel"]');
    await wanSkillLevel.selectOption('7');
    await page.waitForTimeout(100);
    await selectEntryByLabel(page, 'S3');
    await page.waitForTimeout(100);
    ok('switching the FK target to a different skill (ウァン S2→S3) keeps the skill level (SLv7)',
      (await wanSkillLevel.inputValue()) === '7', await wanSkillLevel.inputValue());
  } catch (e) {
    fail++;
    console.log(`FAIL  skill level scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_skill_level.png');
      await page.screenshot({ path: shotPath, fullPage: true });
      console.log(`  screenshot saved: ${shotPath}`);
    } catch (shotErr) {
      console.log(`  screenshot failed: ${shotErr}`);
    }
  } finally {
    await context.close();
  }
}

// P9: 「ホルン」を行に追加すると、そのオペレーターをsourceに持つ条件付きバフ「ホルン」
// (素質「軍事要塞」)が自動でONになり、育成設定(昇進/潜在/モジュール)はカード自身の
// セレクトではなく行の設定にリンクされることを検証する。手動でOFFにした後は行の他
// フィールド変更で再ONにならないこと・行を削除するとカードが自分のセレクトに戻ること・
// リロード後もリンク表示が保たれることも確認する。
// 注: ホルンのモジュールはX(素質「軍事要塞」を強化。Lv3で+31%)とY(素質は強化しないが
// ATK自体はXより高い)の2種がある。汎用の既定モジュールはATKが最大の方(Y)だが、ホルンは
// overrides.yamlの`default_module`でXを初期値にしている(実効ATKはXの方が高い)。そのため
// リンクの既定値は+31%で、Yへ切り替えると素質が強化されないベースE2の値(+23%)になる
// (「モジュールを選んでいてもこのバフには影響しない実効値」のケース)ことも確認する。
async function runLinkedBuffScenario(browser, baseUrl) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto(baseUrl + '/FrameKillCalculator', { waitUntil: 'networkidle' });
    await page.waitForSelector('#add-row-btn', { timeout: 5000 });

    const openGlobalBuffs = async () => {
      if (!(await page.locator('#global-buffs-details').evaluate((el) => el.open))) {
        await page.click('#global-buffs-details summary');
        await page.waitForTimeout(50);
      }
    };
    const selectRowModuleByType = async (typeSuffix) => {
      const select = page.locator('.row-card select[data-field="moduleId"]');
      const options = await select.locator('option').all();
      for (const opt of options) {
        const label = (await opt.innerText()).trim();
        if (label.endsWith(`（${typeSuffix}）`)) {
          await select.selectOption(await opt.getAttribute('value'));
          return;
        }
      }
      throw new Error(`module option ending with (${typeSuffix}) not found`);
    };

    // --- ホルンを追加すると「ホルン」バフが自動でONになる(行は追加直後から展開状態) ---
    await addOperator(page, 'ホルン', null);
    await page.waitForSelector('.row-expanded', { timeout: 5000 });
    await page.fill('#enemy-hp', '100');
    await page.fill('#enemy-def', '0');
    await page.waitForTimeout(150);
    await openGlobalBuffs();
    const hornChip = page.locator('.chip[data-buff-id="horn"]');
    ok('adding a ホルン row auto-enables the ホルン buff chip',
      (await hornChip.getAttribute('aria-pressed')) === 'true');
    const hornCard = page.locator('.cond-source-card[data-buff-id="horn"]');
    ok('ホルン buff card is visible (ON)', (await hornCard.count()) === 1);
    const hornHintText = (await hornCard.locator('.special-hint').innerText()).trim();
    ok('buff card shows "FK行（ホルン）の設定を使用" instead of axis selects',
      hornHintText.includes('FK行（ホルン）の設定を使用'), hornHintText);
    ok('linked buff card has no elite/potential/module selects of its own',
      (await hornCard.locator('select[data-field="elite"]').count()) === 0 &&
      (await hornCard.locator('select[data-field="potential"]').count()) === 0 &&
      (await hornCard.locator('select[data-field="moduleId"]').count()) === 0);
    const hornValueDefault = (await hornCard.locator('.cond-source-value').innerText()).trim();
    ok('ホルン row defaults to module X (overrides default_module) and the buff resolves to +31%',
      hornValueDefault === '+31%', hornValueDefault);

    // --- 行のモジュールをYへ切り替えると、素質「軍事要塞」は強化されず+23%になる ---
    const totalAtModuleX = (await page.locator('#verdict-text').innerText()).trim();
    await selectRowModuleByType('Y');
    await page.waitForTimeout(150);
    await openGlobalBuffs();
    const hornValueModuleY = (await hornCard.locator('.cond-source-value').innerText()).trim();
    ok('switching the row module to Y changes the linked buff value to +23%', hornValueModuleY === '+23%', hornValueModuleY);
    const totalAtModuleY = (await page.locator('#verdict-text').innerText()).trim();
    ok('switching the row module also changes the overall verdict total',
      totalAtModuleX !== totalAtModuleY, { totalAtModuleX, totalAtModuleY });
    await selectRowModuleByType('X');
    await page.waitForTimeout(150);

    // --- 行の昇進をE1に落とすと、モジュール未装備扱い(昇進2未満)になりベースE1の値(+13%)になる ---
    await page.locator('.row-card select[data-field="elite"]').selectOption('1'); // E1
    await page.waitForTimeout(150);
    await openGlobalBuffs();
    const hornValueE1 = (await hornCard.locator('.cond-source-value').innerText()).trim();
    ok('dropping the row elite to E1 changes the linked buff value to +13% (module unusable below E2)',
      hornValueE1 === '+13%', hornValueE1);

    // --- ユーザーが手動でOFFにした後は、行の他フィールド変更で再ONにならない ---
    await hornChip.click();
    await page.waitForTimeout(100);
    ok('turning the buff off manually works', (await hornChip.getAttribute('aria-pressed')) === 'false');
    await page.locator('.row-card select[data-field="elite"]').selectOption('2'); // 昇進を戻す
    await page.waitForTimeout(150);
    ok('changing a row field again does not re-enable the manually-disabled buff (auto-on fires once)',
      (await hornChip.getAttribute('aria-pressed')) === 'false');

    // --- 行を削除すると、カードは自分のセレクトに戻る(リンク解除) ---
    await hornChip.click(); // 再度ON(削除後の比較用)
    await page.waitForTimeout(100);
    await page.click('[data-action="collapse-row"]');
    await page.waitForTimeout(100);
    await page.locator('[data-action="del-row"]').first().click();
    await page.waitForTimeout(100);
    await openGlobalBuffs();
    const hornCardAfterRemove = page.locator('.cond-source-card[data-buff-id="horn"]');
    ok('after removing the ホルン row, the card no longer shows the linked hint',
      (await hornCardAfterRemove.locator('.special-hint').count()) === 0);
    ok('after removing the ホルン row, the card shows its own elite/potential selects again',
      (await hornCardAfterRemove.locator('select[data-field="elite"]').count()) === 1 &&
      (await hornCardAfterRemove.locator('select[data-field="potential"]').count()) === 1);

    // --- 状態はlocalStorageで永続化される(リロード後もリンク表示が保たれる) ---
    await addOperator(page, 'ホルン', null);
    await page.waitForTimeout(500); // 保存のデバウンス(300ms)より長く待つ
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    await openGlobalBuffs();
    const hornCardReloaded = page.locator('.cond-source-card[data-buff-id="horn"]');
    const hornHintReloaded = (await hornCardReloaded.locator('.special-hint').innerText()).trim();
    ok('after reload, the ホルン row + linked buff are both restored',
      hornHintReloaded.includes('FK行（ホルン）の設定を使用'), hornHintReloaded);
  } catch (e) {
    fail++;
    console.log(`FAIL  linked-buff scenario unexpected exception -> ${e && e.stack ? e.stack : e}`);
    try {
      const shotPath = path.join(HERE, 'e2e_fail_linked_buff.png');
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
  await runBuffScenario(browser, baseUrl);
  await runConditionalSourceScenario(browser, baseUrl);
  await runIndividualBuffLevelsScenario(browser, baseUrl);
  await runInspireScenario(browser, baseUrl);
  await runEliteLevelTrustScenario(browser, baseUrl);
  await runSkillLevelScenario(browser, baseUrl);
  await runOldShapeLocalStorageScenario(browser, baseUrl);
  await runPotentialScenario(browser, baseUrl);
  await runFwSpecialScenario(browser, baseUrl);
  await runLinkedBuffScenario(browser, baseUrl);
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
