// トップページの星空（src/api/home/static/planisphere.js）が読む星データ
// src/api/home/static/stars.json を、Yale Bright Star Catalog から生成するスクリプト。
// 生成物は git commit して配信する。ビルド時には実行しない（等級の閾値を変えたいときだけ手で回す）。
//
// 実行方法（node は PATH に無いが実体は存在する。絶対パスで呼ぶこと）:
//   "C:\Program Files\nodejs\node.exe" assets/stars/generate.mjs
//
// データ出典: Yale Bright Star Catalog, 5th Revised Ed.（Hoffleit & Warren 1991）
//   CDS/VizieR V/50  https://cdsarc.cds.unistra.fr/viz-bin/cat/V/50
//   catalog.gz（約570KB）を実行時に取得する。星の位置・等級は事実のデータだが、
//   配布元の利用条件に従うこと（README のデータ出典と、トップのフッターに明記している）。
//
// 出力は [赤経°*10, 赤緯°*10, 実視等級*10, B-V*10] を丸めた整数4つ組の配列。空白なしの1行。
// 明るい星が先頭に来るよう等級の昇順に並べる（描画側が先頭から使える）。
// 座標は J2000。固有運動は無視する（早見盤の見た目には影響しない）。
//
// 固定長カラム（V/50 の ReadMe。1始まり・両端含む）:
//   RAh 76-77, RAm 78-79, RAs 80-83, DE- 84, DEd 85-86, DEm 87-88, DEs 89-90,
//   Vmag 103-107, B-V 110-114

import { gunzipSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

// ============ 微調整はここだけ ============
const CFG = {
  URL: 'https://cdsarc.cds.unistra.fr/ftp/V/50/catalog.gz',
  MAX_VMAG: 5.5,     // これより暗い星は捨てる（肉眼で見える限界に近い。約2900個）
  DEFAULT_BV: 0.6,   // B-V が空欄の星に使う値（太陽に近い白〜黄）
};

const HERE = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(HERE, '..', '..', 'src', 'api', 'home', 'static', 'stars.json');

// 1始まり・両端含むの桁範囲で切り出す
const col = (line, from, to) => line.slice(from - 1, to).trim();

const res = await fetch(CFG.URL);
if (!res.ok) throw new Error(`fetch failed: ${res.status}`);
const text = gunzipSync(Buffer.from(await res.arrayBuffer())).toString('latin1');

const stars = [];
for (const line of text.split('\n')) {
  const rah = col(line, 76, 77), ram = col(line, 78, 79), ras = col(line, 80, 83);
  const vmagStr = col(line, 103, 107);
  const sign = col(line, 84, 84), ded = col(line, 85, 86);
  if (!rah || !vmagStr || !ded) continue;
  const vmag = parseFloat(vmagStr);
  if (!(vmag <= CFG.MAX_VMAG)) continue;
  const ra = (parseInt(rah, 10) + parseInt(ram, 10) / 60 + parseFloat(ras) / 3600) * 15;
  const dem = parseInt(col(line, 87, 88), 10), des = parseInt(col(line, 89, 90), 10);
  let dec = parseInt(ded, 10) + dem / 60 + des / 3600;
  if (sign === '-') dec = -dec;
  const bvStr = col(line, 110, 114);
  const bv = bvStr ? parseFloat(bvStr) : CFG.DEFAULT_BV;
  stars.push([Math.round(ra * 10), Math.round(dec * 10), Math.round(vmag * 10), Math.round(bv * 10)]);
}
stars.sort((a, b) => a[2] - b[2]);

const json = JSON.stringify(stars);
fs.writeFileSync(OUT, json);
console.log(`stars: ${stars.length}, ${Buffer.byteLength(json)} bytes -> ${OUT}`);
