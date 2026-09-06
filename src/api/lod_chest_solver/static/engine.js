/* ============================================================
   幽霊船 宝箱ソルバー — 計算層（DOM非依存。表示は ui.js 側）
   フィードバックは2種類だけ:
     hit  … 「・・・」1桁以上が位置ごと一致
     miss … 「アト○カイ」3桁とも不一致
   ============================================================ */

export const ALL = [];                // "000" 〜 "999"
const DIG = [];                       // [十百, 十, 一] の数値
for (let i = 0; i < 1000; i++) {
  const s = String(i).padStart(3, "0");
  ALL.push(s);
  DIG.push([s.charCodeAt(0) - 48, s.charCodeAt(1) - 48, s.charCodeAt(2) - 48]);
}
// 候補が少ないときは厳密探索で最適手を出す。総候補1000通りの状態から降りてきた
// 集合は形が複雑で探索が重いので、ヒント既知（最大24通り）か、終盤の12通り以下に
// 限って使う。EXACT_SMALL と EXACT_UNIVERSE は用途が違うので統合しないこと
// （ヒント未見モードの終盤で厳密探索を回すと20秒級で固まる）。
export const EXACT_SMALL = 12;
export const EXACT_UNIVERSE = 24;
const EXACT_BUDGET = 120000;          // これを超えたら貪欲手に切り替える

export function ceilLog2(n) {         // n候補を潰すのに最低限必要な手数
  let k = 0;
  while ((1 << k) - 1 < n) k++;
  return k;
}

// 情報量で並んだ最良手が複数（同点）あるとき、桁ごとの差分合計が最小＝
// 物理ダイヤルの回転量が一番少ない候補を優先する最後のタイブレーク。
// pivot（現在ダイヤルに入っている数字）が無い1手目はタイブレークなし＝0扱い。
// ダイヤルは0〜9が輪になっているので、桁ごとの差は循環距離（0と9は1）で数える。
function ringDigitDiff(x, y) {
  const d = Math.abs(x - y);
  return Math.min(d, 10 - d);
}
function digitDist(a, b) {
  const da = DIG[a], db = DIG[b];
  return ringDigitDiff(da[0], db[0]) + ringDigitDiff(da[1], db[1]) + ringDigitDiff(da[2], db[2]);
}

// 回転量が同じ候補同士（pivotの上下に等距離で存在する数字）の並べ替え用。
// 「桁の数値が小さい方」で決めると0付近だけ上向き・他は下向きになって
// 回す向きがバラつくので、上向き（増加方向。9の次は0）で到達する桁数が
// 多い方を優先する。下向きでしか届かない桁数を数えて少ない方を勝たせる。
function digitDownCount(a, b) {
  const da = DIG[a], db = DIG[b];
  let down = 0;
  for (let i = 0; i < 3; i++) {
    const up = (da[i] - db[i] + 10) % 10;
    if (up > (db[i] - da[i] + 10) % 10) down++;   // ちょうど半周(5)は上向き扱い
  }
  return down;
}

// pivotへの寄り具合の比較。距離が同点なら上向き優先。
function closerToPivot(g, cur, pivot) {
  const dg = digitDist(g, pivot), dc = digitDist(cur, pivot);
  if (dg !== dc) return dg < dc;
  return digitDownCount(g, pivot) < digitDownCount(cur, pivot);
}

export function hintCandidates(hints) { // 4数字のうち3つの順列（重複数字も正しく扱う）
  const set = new Set();
  for (let a = 0; a < 4; a++)
    for (let b = 0; b < 4; b++)
      for (let c = 0; c < 4; c++)
        if (a !== b && b !== c && a !== c)
          set.add(hints[a] + hints[b] + hints[c]);
  return [...set].sort().map(s => parseInt(s, 10));
}

// 昇順ソート用: .key配列を持つ要素同士を先頭から辞書式に比較する。
function cmpKeys(a, b) {
  const ka = a.key, kb = b.key;
  for (let i = 0; i < ka.length; i++) if (ka[i] !== kb[i]) return ka[i] - kb[i];
  return 0;
}

/* --- 貪欲: 候補が多いときのヒューリスティック ---
   tries === null（無制限モード）: 最悪ケースで残り候補が最小になる入力を選ぶ
   （最悪手数の最小化。従来どおり）。
   tries が数値（制限モード）: 残りtries回で開けられる候補数の上界
     score(g) = [gが候補自身か] + min(hit側候補数, 2^(tries-1)-1) + min(miss側候補数, 同上)
   を最優先で使う近似（真の V(S,tries) は1000通りの集合では計算できないため上界で代用）。
   tries が大きいうちは上界が効かず従来と同じ手が出て、tries が小さくなるにつれ
   候補そのものを撃つ手へ自然に寄る。 */
function greedyBest(cands, top, pivot, tries) {
  const n = cands.length, out = [];
  const half = tries === null ? Infinity : Math.max(0, (1 << Math.max(tries - 1, 0)) - 1);
  for (let g = 0; g < 1000; g++) {
    const [ga, gb, gc] = DIG[g];
    let nh = 0, nm = 0, self = false;
    for (let k = 0; k < n; k++) {
      const c = cands[k];
      if (c === g) { self = true; continue; }
      const d = DIG[c];
      if (d[0] === ga || d[1] === gb || d[2] === gc) nh++; else nm++;
    }
    if (nh === n || nm === n) continue;          // 情報ゼロの入力は捨てる
    const tieKey = [Math.max(ceilLog2(nh), ceilLog2(nm)), Math.max(nh, nm), self ? 0 : 1,
                    pivot === null ? 0 : digitDist(g, pivot),
                    pivot === null ? 0 : digitDownCount(g, pivot)];
    const key = tries === null
      ? tieKey
      : [-((self ? 1 : 0) + Math.min(nh, half) + Math.min(nm, half)), ...tieKey];
    out.push({ guess: g, nh, nm, key });
  }
  out.sort(cmpKeys);
  return out.slice(0, top);
}

/* --- 厳密探索: 候補が少ないときの最適手（ミニマックス or 回数制限内成功率） --- */
function exactBest(cands, top, pivot, tries) {
  const n = cands.length;
  let budget = EXACT_BUDGET;                        // n <= 24 なのでビットは32bitに収まる
  const hitMask = new Int32Array(1000);
  const selfBit = new Int32Array(1000);
  const pos = new Map();
  cands.forEach((c, i) => pos.set(c, i));
  for (let g = 0; g < 1000; g++) {
    const [ga, gb, gc] = DIG[g];
    let m = 0;
    for (let k = 0; k < n; k++) {
      const d = DIG[cands[k]];
      if (d[0] === ga || d[1] === gb || d[2] === gc) m |= 1 << k;
    }
    hitMask[g] = m;
    selfBit[g] = pos.has(g) ? (1 << pos.get(g)) : 0;
  }
  const FULL = n === 32 ? -1 : (1 << n) - 1;
  const splitCache = new Map();
  const pc = x => { let c = 0; while (x) { x &= x - 1; c++; } return c; };

  // 候補集合への効き方が同じ入力は完全に等価なので、先に1つへまとめておく。
  // 代表は各同値クラスの中でpivotに一番近いものを選ぶ（同値クラス内の他の値は
  // splits()に一切出てこなくなるので、ここで選ばないとタイブレークが効かない）。
  const seenG = new Map();                          // key -> 代表g
  for (let g = 0; g < 1000; g++) {
    const key = hitMask[g] * 33554432 + selfBit[g];
    const cur = seenG.get(key);
    if (cur === undefined || (pivot !== null && closerToPivot(g, cur, pivot))) {
      seenG.set(key, g);
    }
  }
  const uniq = [...seenG.values()];

  function splits(S) {
    const cached = splitCache.get(S);
    if (cached) return cached;
    const size = pc(S), seen = new Set(), out = [];
    for (const g of uniq) {
      const hit = S & hitMask[g] & ~selfBit[g];
      const miss = S & ~hitMask[g];
      const key = hit * 33554432 + miss;         // 25bitずらして一意化
      if (seen.has(key)) continue;
      seen.add(key);
      const nh = pc(hit), nm = pc(miss);
      if ((nh === size || nm === size) && !(S & selfBit[g])) continue;
      out.push({ guess: g, hit, miss, nh, nm });
    }
    out.sort((a, b) =>
      Math.max(ceilLog2(a.nh), ceilLog2(a.nm)) - Math.max(ceilLog2(b.nh), ceilLog2(b.nm))
      || Math.max(a.nh, a.nm) - Math.max(b.nh, b.nm));
    splitCache.set(S, out);
    return out;
  }

  /* --- 最悪手数（ミニマックス）。無制限モードの本体、制限モードではタイブレーク3で使う --- */
  const lo = new Map(), hi = new Map();
  function feasible(S, b) {
    if (--budget < 0) throw new RangeError("budget");
    const size = pc(S);
    if (size <= 1) return b >= size;
    if (b <= 1 || size > (1 << b) - 1) return false;
    if (b >= (hi.get(S) ?? 99)) return true;
    if (b < (lo.get(S) ?? 0)) return false;
    const opts = splits(S);
    if (!opts.length) { lo.set(S, 99); return false; }
    const need = Math.max(ceilLog2(opts[0].nh), ceilLog2(opts[0].nm)) + 1;
    if (need > (lo.get(S) ?? 0)) lo.set(S, need);
    if (b < lo.get(S)) return false;
    for (const o of opts) {
      if (feasible(o.miss, b - 1) && feasible(o.hit, b - 1)) {
        hi.set(S, Math.min(hi.get(S) ?? 99, b));
        return true;
      }
    }
    lo.set(S, Math.max(lo.get(S) ?? 0, b + 1));
    return false;
  }
  function worstDepth(S) {
    let d = ceilLog2(pc(S));
    while (d < 30 && !feasible(S, d)) d++;
    return d;
  }

  if (tries === null) {
    try {
      const target = worstDepth(FULL);
      const good = [];
      for (const o of splits(FULL)) {
        if (Math.max(ceilLog2(o.nh), ceilLog2(o.nm)) + 1 > target) continue;
        if (feasible(o.miss, target - 1) && feasible(o.hit, target - 1)) {
          good.push({ guess: o.guess, nh: o.nh, nm: o.nm,
                      key: [Math.max(o.nh, o.nm), (FULL & selfBit[o.guess]) ? 0 : 1,
                            pivot === null ? 0 : digitDist(o.guess, pivot),
                            pivot === null ? 0 : digitDownCount(o.guess, pivot)] });
        }
      }
      good.sort(cmpKeys);
      return { list: good.slice(0, top), depth: target, cover: null };
    } catch (e) {
      if (!(e instanceof RangeError)) throw e;
      return null;                       // 打ち切り。貪欲手にフォールバック
    }
  }

  /* --- 制限モード: V(S,b) = 残りb回で開けられる候補の最大数 ---
     V(S,0) = 0
     V(S,b) = max over splits(S) of [g∈S] + V(hit,b-1) + V(miss,b-1)
     splits() の hit は「hitしたが開かなかった」集合なのでg自身を含まない
     （g∈S の成功はhit/miss分岐と独立に、その場でこの入力が正解だった場合の1件として加算）。 */
  const cov = new Map();               // key: S*32+b -> V(S,b)
  function coverage(S, b) {
    if (b <= 0) return 0;
    const size = pc(S);
    if (size === 0) return 0;
    if (size === 1) return 1;          // 残り1つは次の1回で必ず当たる
    const key = S * 32 + b;
    const cached = cov.get(key);
    if (cached !== undefined) return cached;
    if (--budget < 0) throw new RangeError("budget");
    const cap = Math.min(size, (1 << b) - 1);       // 情報理論的な上限（20の扉問題と同じ形）
    const half = (1 << (b - 1)) - 1;
    const ranked = splits(S).map(o => {
      const self = (S & selfBit[o.guess]) ? 1 : 0;
      return { o, self, upper: self + Math.min(o.nh, half) + Math.min(o.nm, half) };
    }).sort((a, c) => c.upper - a.upper);            // 上界の大きい順に評価してcap到達を早める
    let best = 0;
    for (const { o, self, upper } of ranked) {
      if (upper <= best) break;        // 降順なのでこれ以降も現在の最善を超えられない
      const v = self + coverage(o.hit, b - 1) + coverage(o.miss, b - 1);
      if (v > best) best = v;
      if (best >= cap) break;
    }
    cov.set(key, best);
    return best;
  }

  try {
    const opts = splits(FULL);
    if (!opts.length) return null;
    const bestCover = coverage(FULL, tries);
    const good = [];
    for (const o of opts) {
      const self = (FULL & selfBit[o.guess]) ? 1 : 0;
      const c0 = self + coverage(o.hit, tries - 1) + coverage(o.miss, tries - 1);
      if (c0 !== bestCover) continue;                // タイブレーク1: 制限内成功率が最大の手だけ残す
      const vec = [];                                 // タイブレーク2: 残りj回時点の成功率を辞書式に最大化
      for (let j = tries - 1; j >= 1; j--) vec.push(self + coverage(o.hit, j - 1) + coverage(o.miss, j - 1));
      const worst = 1 + Math.max(worstDepth(o.hit), worstDepth(o.miss)); // タイブレーク3: 最悪手数
      good.push({
        guess: o.guess, nh: o.nh, nm: o.nm,
        key: [...vec.map(v => -v), worst,
              pivot === null ? 0 : digitDist(o.guess, pivot),
              pivot === null ? 0 : digitDownCount(o.guess, pivot)]
      });
    }
    good.sort(cmpKeys);
    return { list: good.slice(0, top), depth: null, cover: bestCover };
  } catch (e) {
    if (!(e instanceof RangeError)) throw e;
    return null;                       // 打ち切り。貪欲手にフォールバック
  }
}

// tries: 残り入力回数。null なら無制限（S/L前提。最悪手数を最小化する従来の挙動）。
// 数値を渡すと、その回数以内に開錠できる確率を最大化する（早く当たりを引きにいく）。
export function bestGuesses(cands, universeSize, top, pivot = null, tries = null) {
  if (cands.length <= EXACT_SMALL || universeSize <= EXACT_UNIVERSE) {
    const r = exactBest(cands, top, pivot, tries);
    if (r && r.list.length) return r;
  }
  return { list: greedyBest(cands, top, pivot, tries), depth: null, cover: null };
}

export function narrow(cands, guess, isHit) {
  const [ga, gb, gc] = DIG[guess];
  return cands.filter(c => {
    if (c === guess) return false;               // 開かなかった＝その数字ではない
    const d = DIG[c];
    const h = (d[0] === ga || d[1] === gb || d[2] === gc);
    return isHit ? h : !h;
  });
}
