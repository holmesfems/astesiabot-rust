// トップページの星空（src/api/home/static/planisphere.js）が読む星データ
// src/api/home/static/stars.json と、黄道十二星座の線 zodiac_lines.json を、
// Yale Bright Star Catalog から生成するスクリプト。
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
// zodiac_lines.json は [始点の赤経°*10, 赤緯°*10, 終点の赤経°*10, 赤緯°*10] の整数4つ組の配列。
// 星座の線の結び方に公式の決まりは無く、配布されている線データにはそれぞれライセンスがあるので、
// 下の ZODIAC はこのサイト用に一から書き起こした簡素な線（バイエル名で指定し、座標は星表から引く）。
//
// 固定長カラム（V/50 の ReadMe。1始まり・両端含む）:
//   Name 5-14（Bayer 8-10、その上付き番号 11、星座略号 12-14）,
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

// 黄道十二星座の線。星は「バイエル符号(+上付き番号) 星座略号」で書き、各配列は一筆書きでつなぐ点列。
// 星座ごとに複数の点列を持てる（枝分かれは別の点列にする）。
const ZODIAC = {
  Ari: [['Alp Ari', 'Bet Ari', 'Gam2 Ari'], ['Alp Ari', 'Del Ari']],
  Tau: [['Zet Tau', 'Alp Tau', 'The2 Tau', 'Gam Tau', 'Del1 Tau', 'Eps Tau', 'Bet Tau'], ['Gam Tau', 'Lam Tau']],
  Gem: [['Alp Gem', 'Tau Gem', 'Eps Gem', 'Mu Gem', 'Eta Gem'], ['Bet Gem', 'Kap Gem', 'Del Gem', 'Zet Gem', 'Gam Gem'], ['Tau Gem', 'Kap Gem']],
  Cnc: [['Iot Cnc', 'Gam Cnc', 'Del Cnc', 'Bet Cnc'], ['Del Cnc', 'Alp Cnc']],
  Leo: [['Eps Leo', 'Mu Leo', 'Zet Leo', 'Gam1 Leo', 'Eta Leo', 'Alp Leo', 'The Leo', 'Bet Leo', 'Del Leo', 'Gam1 Leo']],
  Vir: [['Bet Vir', 'Eta Vir', 'Gam Vir', 'Del Vir', 'Eps Vir'], ['Gam Vir', 'The Vir', 'Alp Vir', 'Zet Vir', 'Del Vir']],
  Lib: [['Sig Lib', 'Alp2 Lib', 'Bet Lib', 'Gam Lib', 'Ups Lib'], ['Alp2 Lib', 'Gam Lib']],
  Sco: [['Bet1 Sco', 'Del Sco', 'Pi Sco'], ['Del Sco', 'Sig Sco', 'Alp Sco', 'Tau Sco', 'Eps Sco', 'Mu1 Sco', 'Zet2 Sco', 'Eta Sco', 'The Sco', 'Iot1 Sco', 'Kap Sco', 'Lam Sco', 'Ups Sco']],
  Sgr: [['Gam2 Sgr', 'Del Sgr', 'Eps Sgr', 'Gam2 Sgr'], ['Del Sgr', 'Lam Sgr', 'Phi Sgr', 'Del Sgr'], ['Eps Sgr', 'Zet Sgr', 'Phi Sgr', 'Sig Sgr', 'Tau Sgr', 'Zet Sgr']],
  Cap: [['Alp2 Cap', 'Bet Cap', 'Psi Cap', 'Ome Cap', 'Zet Cap', 'Del Cap', 'Gam Cap', 'The Cap', 'Bet Cap']],
  Aqr: [['Eps Aqr', 'Bet Aqr', 'Alp Aqr', 'Gam Aqr', 'Zet1 Aqr', 'Eta Aqr'], ['Zet1 Aqr', 'Pi Aqr'], ['Alp Aqr', 'The Aqr', 'Lam Aqr', 'Del Aqr']],
  Psc: [['Gam Psc', 'Kap Psc', 'Lam Psc', 'Iot Psc', 'The Psc', 'Gam Psc'], ['Iot Psc', 'Ome Psc', 'Del Psc', 'Eps Psc', 'Zet Psc', 'Mu Psc', 'Nu Psc', 'Alp Psc', 'Omi Psc', 'Eta Psc']],
};

const HERE = path.dirname(fileURLToPath(import.meta.url));
const STATIC = path.resolve(HERE, '..', '..', 'src', 'api', 'home', 'static');
const OUT = path.join(STATIC, 'stars.json');
const OUT_LINES = path.join(STATIC, 'zodiac_lines.json');

// 1始まり・両端含むの桁範囲で切り出す
const col = (line, from, to) => line.slice(from - 1, to).trim();

const res = await fetch(CFG.URL);
if (!res.ok) throw new Error(`fetch failed: ${res.status}`);
const text = gunzipSync(Buffer.from(await res.arrayBuffer())).toString('latin1');

const stars = [];
const byName = new Map();   // "Alp Ari" → [赤経°, 赤緯°]（線の端点は等級で絞らずに引く）
for (const line of text.split('\n')) {
  const rah = col(line, 76, 77), ram = col(line, 78, 79), ras = col(line, 80, 83);
  const vmagStr = col(line, 103, 107);
  const sign = col(line, 84, 84), ded = col(line, 85, 86);
  if (!rah || !vmagStr || !ded) continue;
  const ra = (parseInt(rah, 10) + parseInt(ram, 10) / 60 + parseFloat(ras) / 3600) * 15;
  const dem = parseInt(col(line, 87, 88), 10), des = parseInt(col(line, 89, 90), 10);
  let dec = parseInt(ded, 10) + dem / 60 + des / 3600;
  if (sign === '-') dec = -dec;
  const bayer = col(line, 8, 10);
  if (bayer) byName.set(`${bayer}${col(line, 11, 11)} ${col(line, 12, 14)}`, [ra, dec]);
  const vmag = parseFloat(vmagStr);
  if (!(vmag <= CFG.MAX_VMAG)) continue;
  const bvStr = col(line, 110, 114);
  const bv = bvStr ? parseFloat(bvStr) : CFG.DEFAULT_BV;
  stars.push([Math.round(ra * 10), Math.round(dec * 10), Math.round(vmag * 10), Math.round(bv * 10)]);
}
stars.sort((a, b) => a[2] - b[2]);

const json = JSON.stringify(stars);
fs.writeFileSync(OUT, json);
console.log(`stars: ${stars.length}, ${Buffer.byteLength(json)} bytes -> ${OUT}`);

// 点列を隣り合う2点の線分に分ける。名前が星表に無ければ書き間違いなので止める。
const lines = [];
for (const [con, paths] of Object.entries(ZODIAC)) {
  for (const names of paths) {
    const pts = names.map((n) => {
      const p = byName.get(n);
      if (!p) throw new Error(`${con}: "${n}" が星表に無い`);
      return p;
    });
    for (let i = 1; i < pts.length; i++) {
      const [a, b] = [pts[i - 1], pts[i]];
      lines.push([a[0], a[1], b[0], b[1]].map((v) => Math.round(v * 10)));
    }
  }
}
const linesJson = JSON.stringify(lines);
fs.writeFileSync(OUT_LINES, linesJson);
console.log(`zodiac lines: ${lines.length}, ${Buffer.byteLength(linesJson)} bytes -> ${OUT_LINES}`);
