// 各ツールの e2e.mjs が使う Playwright を探す。
//
// Playwright はプロジェクト直下ではなく npx のキャッシュ（%LOCALAPPDATA%\npm-cache\_npx\<hash>\node_modules\playwright）
// に入っていて、版ごとに要るブラウザ（%LOCALAPPDATA%\ms-playwright\chromium-<番号>）が違う。
// 最初に見つかった版を使うと、ブラウザが入っていない古い版を拾って launch で落ちるので、
// Chromium の実行ファイルが実在する版のうち一番新しいものを選ぶ。

import { createRequire } from 'node:module';
import path from 'node:path';
import fs from 'node:fs';

const require = createRequire(import.meta.url);

function npxCacheRoot() {
  return path.join(
    process.env.LOCALAPPDATA || path.join(process.env.USERPROFILE || '', 'AppData', 'Local'),
    'npm-cache',
    '_npx'
  );
}

// "1.63.0" / "1.64.0-alpha-…" を [1, 63, 0, プレリリースなら0・正式なら1] にして比べる。
function versionKey(version) {
  const [main, pre] = String(version).split('-', 2);
  return [...main.split('.').map((n) => Number(n) || 0), pre ? 0 : 1];
}

function compareVersionDesc(a, b) {
  const ka = versionKey(a.version);
  const kb = versionKey(b.version);
  for (let i = 0; i < Math.max(ka.length, kb.length); i++) {
    const d = (kb[i] ?? 0) - (ka[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

function load(dirOrName) {
  try {
    const pw = require(dirOrName);
    const pkg = require(path.join(path.dirname(require.resolve(dirOrName)), 'package.json'));
    return { pw, version: pkg.version, where: path.dirname(require.resolve(dirOrName)) };
  } catch (e) {
    return null;
  }
}

function browserInstalled(pw) {
  try {
    return fs.existsSync(pw.chromium.executablePath());
  } catch (e) {
    return false;
  }
}

export function resolvePlaywright() {
  const candidates = [];
  // 通常の解決（NODE_PATH を含む）で見つかるもの
  const normal = load('playwright');
  if (normal) candidates.push(normal);

  const root = npxCacheRoot();
  if (fs.existsSync(root)) {
    for (const d of fs.readdirSync(root, { withFileTypes: true })) {
      if (!d.isDirectory()) continue;
      const dir = path.join(root, d.name, 'node_modules', 'playwright');
      if (!fs.existsSync(path.join(dir, 'package.json'))) continue;
      const c = load(dir);
      if (c && !candidates.some((x) => x.where === c.where)) candidates.push(c);
    }
  }

  const usable = candidates.filter((c) => browserInstalled(c.pw)).sort(compareVersionDesc);
  if (usable.length > 0) {
    console.log(`playwright ${usable[0].version} を使います (${usable[0].where})`);
    return usable[0].pw;
  }

  console.error('ブラウザの入っている playwright が見つかりません。');
  if (candidates.length > 0) {
    console.error('  見つかった版（ブラウザ無し）: ' + candidates.map((c) => c.version).join(', '));
    console.error('  例: npx playwright install chromium');
  } else {
    console.error('  npx 経由でのインストール例: npx playwright install chromium');
  }
  console.error('  探索したパス: ' + root);
  process.exit(1);
}
