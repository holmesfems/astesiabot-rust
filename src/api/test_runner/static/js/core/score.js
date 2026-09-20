import { P } from '../constants/i18n.js';

/* =========================================================================
   スコア / ペース判定 / 乱数ユーティリティ（DOM非依存）
   ========================================================================= */
export function classifyPace(avgSec) {
  if (avgSec < 20) return 'fast';
  if (avgSec < 90) return 'steady';
  return 'careful';
}

export function timePraisePoolFor(pace) {
  if (pace === 'fast') return P.TIME_PRAISE_FAST;
  if (pace === 'steady') return P.TIME_PRAISE_STEADY;
  return P.TIME_PRAISE_CAREFUL;
}

export function timeBonusFor(pace) {
  if (pace === 'fast') return 300;
  if (pace === 'steady') return 200;
  return 250;
}

export function formatDuration(ms) {
  var totalSec = Math.max(0, Math.round((ms || 0) / 1000));
  var h = Math.floor(totalSec / 3600);
  var m = Math.floor((totalSec % 3600) / 60);
  var s = totalSec % 60;
  var pad2 = function (n) { return (n < 10 ? '0' : '') + n; };
  if (h > 0) return h + ':' + pad2(m) + ':' + pad2(s);
  return m + ':' + pad2(s);
}

export function pickRandom(arr) { return arr[Math.floor(Math.random() * arr.length)]; }

export function shuffleArray(arr) {
  var a = arr.slice();
  for (var i = a.length - 1; i > 0; i--) {
    var j = Math.floor(Math.random() * (i + 1));
    var tmp = a[i]; a[i] = a[j]; a[j] = tmp;
  }
  return a;
}

/* 重複なしで最大 n 個を選ぶ（arr.length < n の場合は足りる分だけ返す） */
export function pickDistinct(arr, n) {
  return shuffleArray(arr).slice(0, Math.min(n, arr.length));
}

export function getPraiseForCombo(combo) {
  var idx = Math.min(combo - 1, P.PRAISE_POOL.length - 1);
  var jitter = Math.floor(Math.random() * 3);
  idx = Math.max(0, idx - jitter);
  return P.PRAISE_POOL[idx];
}
