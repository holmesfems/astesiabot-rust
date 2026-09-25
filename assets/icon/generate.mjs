// サイトアイコン一式（src/api/site_icons/static/）を元画像から生成するスクリプト。
// 生成物は git commit して配信する。ビルド時には実行しない（元画像を差し替えたときだけ手で回す）。
//
// 実行方法（node は PATH に無いが実体は存在する。絶対パスで呼ぶこと）:
//   "C:\Program Files\nodejs\node.exe" assets/icon/generate.mjs
//
// 元画像:
//   astesia-bot-icon-1024.png        … 通常版。180px 以上（apple-touch-icon / PWA / OGP）に使う
//   astesia-bot-icon-simple.svg      … simple版。48px 以下（ファビコン）と maskable に使う。
//                                      デザインツール書き出しの SVG のテンプレート変数を
//                                      解決して背景を足したもの
//   astesia-bot-icon-simple-1024.png … simple版を Chromium で描いた確認用。生成には使わない
//
// デザインツールの PNG 書き出しは `<use fill="…">` の fill を落として星を黒く塗ったので、
// SVG は必ずブラウザ（Chromium）で描く。

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const require = createRequire(import.meta.url);
const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '..', '..');
const OUT_DIR = path.join(REPO_ROOT, 'src', 'api', 'site_icons', 'static');

// test_runner/e2e.mjs と同じ解決方法（npx キャッシュのハッシュ名は決め打ちしない）。
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
    for (const d of fs.readdirSync(npxCacheRoot, { withFileTypes: true })) {
      const candidate = path.join(npxCacheRoot, d.name, 'node_modules', 'playwright');
      if (d.isDirectory() && fs.existsSync(path.join(candidate, 'package.json'))) {
        try {
          return require(candidate);
        } catch (e) {
          // 次の候補へ
        }
      }
    }
  }
  console.error('playwright が見つかりません。 npx playwright install chromium でインストールしてください。');
  process.exit(1);
}

const GROUND = '#101533';
const simpleSvg = fs.readFileSync(path.join(HERE, 'astesia-bot-icon-simple.svg'), 'utf8');
const fullPngDataUrl =
  'data:image/png;base64,' + fs.readFileSync(path.join(HERE, 'astesia-bot-icon-1024.png')).toString('base64');

/** SVG を size×size でネイティブに描いて PNG にする（縮小ではなく各サイズでベクタ描画）。 */
async function renderSvg(page, size, scale = 1) {
  const inner = size * scale;
  const offset = (size - inner) / 2;
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(
    `<html><body style="margin:0;background:${GROUND}">` +
      `<div style="position:absolute;left:${offset}px;top:${offset}px;width:${inner}px;height:${inner}px">` +
      simpleSvg.replace('<svg ', `<svg width="${inner}" height="${inner}" `) +
      `</div></body></html>`
  );
  return page.screenshot({ clip: { x: 0, y: 0, width: size, height: size } });
}

/** 通常版 PNG を canvas で半分ずつ縮小してから目的サイズへ（一気に縮めると細い線がちらつく）。 */
async function downscalePng(page, size) {
  await page.setContent('<html><body></body></html>');
  const dataUrl = await page.evaluate(
    async ({ src, size }) => {
      const img = new Image();
      img.src = src;
      await img.decode();
      let cur = img;
      let w = img.width;
      while (w / 2 >= size) {
        w = Math.floor(w / 2);
        const c = document.createElement('canvas');
        c.width = c.height = w;
        const ctx = c.getContext('2d');
        ctx.imageSmoothingQuality = 'high';
        ctx.drawImage(cur, 0, 0, w, w);
        cur = c;
      }
      const out = document.createElement('canvas');
      out.width = out.height = size;
      const ctx = out.getContext('2d');
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(cur, 0, 0, size, size);
      return out.toDataURL('image/png');
    },
    { src: fullPngDataUrl, size }
  );
  return Buffer.from(dataUrl.split(',')[1], 'base64');
}

/** PNG を埋め込む形式の ICO（Vista 以降の全ブラウザが読める）を組み立てる。 */
function buildIco(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = 6 + dir.length;
  entries.forEach(({ size, png }, i) => {
    const o = i * 16;
    dir.writeUInt8(size >= 256 ? 0 : size, o);
    dir.writeUInt8(size >= 256 ? 0 : size, o + 1);
    dir.writeUInt8(0, o + 2);
    dir.writeUInt8(0, o + 3);
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += png.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.png)]);
}

const { chromium } = resolvePlaywright();
const browser = await chromium.launch();
try {
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const write = (name, buf) => {
    fs.writeFileSync(path.join(OUT_DIR, name), buf);
    console.log(`${name}  ${buf.length} bytes`);
  };

  const icoEntries = [];
  for (const size of [16, 32, 48]) {
    icoEntries.push({ size, png: await renderSvg(page, size) });
  }
  write('favicon.ico', buildIco(icoEntries));
  write('favicon.svg', Buffer.from(simpleSvg));

  write('apple-touch-icon.png', await downscalePng(page, 180));
  write('icon-192.png', await downscalePng(page, 192));
  write('icon-512.png', await downscalePng(page, 512));

  // maskable: Android は中心から半径 40%（= 直径 80%）の円をセーフゾーンとして保証する。
  // simple版の外周リングの外縁は 512 中 半径232 なので、204.8/232 ≒ 0.88 以下に縮めれば
  // 円クロップでもリングが切れない。少し余裕を持たせて 0.86。
  write('icon-maskable-512.png', await renderSvg(page, 512, 0.86));
} finally {
  await browser.close();
}
