import { T, P } from '../../constants/i18n.js';
import { CONFETTI_COLORS } from '../../constants/config.js';
import { el, reducedMotionOS, showScoreFloat } from '../dom.js';
import { pickRandom, pickDistinct, classifyPace, timeBonusFor, timePraisePoolFor, getPraiseForCombo, formatDuration } from '../../core/score.js';
import { state, currentItem, countStatusesInSection, computeSectionDurationMs, saveSession, computeOverallStats, computeRank, computeTotalDurationMs } from '../../core/state.js';
import { stopStepTimer, renderStep, goToResultScreen } from '../renderer.js';
import { isLastSection } from '../flow.js';

/* =========================================================================
   演出: OK セレブレーション（紙吹雪 / フラッシュ / 褒め言葉 / スコア）
   節完了 / グランドフィナーレ 演出
   ========================================================================= */
var confettiCanvas;
var confettiCtx;
var particles = [];
var confettiRunning = false;

function resizeConfettiCanvas() {
  confettiCanvas.width = window.innerWidth;
  confettiCanvas.height = window.innerHeight;
}

// confettiCanvas は DOM 要素に依存するため boot() 経由の init() から呼ぶ（トップレベル副作用にしない）
export function initConfetti() {
  confettiCanvas = el('confetti-canvas');
  confettiCtx = confettiCanvas.getContext('2d');
  window.addEventListener('resize', resizeConfettiCanvas);
  resizeConfettiCanvas();
}

export function spawnConfetti(count, originYRatio) {
  if (reducedMotionOS) count = Math.min(count, 12);
  var w = confettiCanvas.width, h = confettiCanvas.height;
  for (var i = 0; i < count; i++) {
    particles.push({
      x: Math.random() * w,
      y: h * (originYRatio || 0.15) * Math.random(),
      vx: (Math.random() - 0.5) * 4,
      vy: 2 + Math.random() * 4,
      size: 5 + Math.random() * 7,
      rot: Math.random() * Math.PI * 2,
      rotSpeed: (Math.random() - 0.5) * 0.3,
      color: pickRandom(CONFETTI_COLORS),
      shape: Math.random() < 0.5 ? 'rect' : 'circle',
      life: 0,
      maxLife: 90 + Math.random() * 60
    });
  }
  if (!confettiRunning) { confettiRunning = true; requestAnimationFrame(confettiLoop); }
}

function confettiLoop() {
  confettiCtx.clearRect(0, 0, confettiCanvas.width, confettiCanvas.height);
  var alive = [];
  for (var i = 0; i < particles.length; i++) {
    var p = particles[i];
    p.x += p.vx;
    p.y += p.vy;
    p.vy += 0.03;
    p.rot += p.rotSpeed;
    p.life++;
    if (p.y < confettiCanvas.height + 20 && p.life < p.maxLife) {
      alive.push(p);
      confettiCtx.save();
      confettiCtx.translate(p.x, p.y);
      confettiCtx.rotate(p.rot);
      confettiCtx.fillStyle = p.color;
      confettiCtx.globalAlpha = Math.max(0, 1 - p.life / p.maxLife);
      if (p.shape === 'rect') {
        confettiCtx.fillRect(-p.size / 2, -p.size / 4, p.size, p.size / 2);
      } else {
        confettiCtx.beginPath();
        confettiCtx.arc(0, 0, p.size / 2.4, 0, Math.PI * 2);
        confettiCtx.fill();
      }
      confettiCtx.restore();
    }
  }
  particles = alive;
  if (particles.length > 0) {
    requestAnimationFrame(confettiLoop);
  } else {
    confettiRunning = false;
    confettiCtx.clearRect(0, 0, confettiCanvas.width, confettiCanvas.height);
  }
}

function triggerFlash() {
  if (reducedMotionOS) return;
  var fx = el('flash-effect');
  fx.classList.remove('flash');
  void fx.offsetWidth;
  fx.classList.add('flash');
}

function showPraisePop(text) {
  var node = el('praise-pop');
  node.textContent = text;
  node.classList.remove('show');
  void node.offsetWidth;
  node.classList.add('show');

  var backdrop = el('praise-backdrop');
  if (backdrop) {
    backdrop.classList.remove('show');
    void backdrop.offsetWidth;
    backdrop.classList.add('show');
  }
}

export function fireOkCelebration(gained, combo) {
  var count = 80 + Math.floor(Math.random() * 70);
  spawnConfetti(count, 0.1);
  triggerFlash();
  showPraisePop(getPraiseForCombo(combo));
  var scoreText = '+' + gained + ' pt' + (combo >= 2 ? '　COMBO x' + combo + '!' : '');
  showScoreFloat(scoreText);
}

export function showSectionCompleteOverlay() {
  stopStepTimer();
  var item = currentItem();
  var sIdx = item.sectionIndex;
  var counts = countStatusesInSection(sIdx);

  spawnConfetti(150, 0.05);
  el('sc-heading').textContent = T.sectionCompleteHeading(sIdx + 1);
  el('sc-title').textContent = T.sectionTitleReveal(pickRandom(P.SECTION_TITLE_POOL));
  el('sc-ok').textContent = counts.ok;
  el('sc-ng').textContent = counts.ng;
  var scPraises = pickDistinct(P.SECTION_PRAISE_POOL, 3);
  el('sc-praise1').textContent = scPraises[0] || '';
  el('sc-praise2').textContent = scPraises[1] || '';
  el('sc-praise3').textContent = scPraises[2] || '';

  var sectionDurationMs = computeSectionDurationMs(sIdx);
  var itemCountForPace = counts.total || 1;
  var avgSec = (sectionDurationMs / itemCountForPace) / 1000;
  var pace = classifyPace(avgSec);
  var bonus = timeBonusFor(pace);
  state.score += bonus;
  el('sc-duration').textContent = formatDuration(sectionDurationMs);
  el('sc-time-bonus').textContent = '+' + bonus + ' pt';
  el('sc-time-praise').textContent = pickRandom(timePraisePoolFor(pace));
  saveSession();

  var lastSection = isLastSection();
  el('sc-next-btn').textContent = lastSection ? T.toFinaleLabel : T.nextSectionLabel;
  el('overlay-section-complete').hidden = false;
  currentOverlayPrimaryAction = function () {
    el('overlay-section-complete').hidden = true;
    if (lastSection) {
      showFinaleOverlay();
    } else {
      state.pointer++;
      saveSession();
      renderStep(true); // re-enables ok/ng/back buttons
    }
    state.transitioning = false; // release the guard set in recordResult() now that we've fully moved past this item
  };
}

export function showFinaleOverlay() {
  stopStepTimer();
  spawnConfetti(150, 0.02);
  setTimeout(function () { spawnConfetti(120, 0.02); }, 300);
  setTimeout(function () { spawnConfetti(120, 0.02); }, 650);
  var stats = computeOverallStats();
  var rate = stats.total > 0 ? Math.round((stats.ok / stats.total) * 100) : 0;
  el('finale-rank').textContent = computeRank(stats);
  el('finale-total').textContent = stats.total;
  el('finale-ok').textContent = stats.ok;
  el('finale-ng').textContent = stats.ng;
  el('finale-rate').textContent = rate + '%';
  var finalePraises = pickDistinct(P.FINALE_PRAISE_POOL, 3);
  el('finale-praise1').textContent = finalePraises[0] || '';
  el('finale-praise2').textContent = finalePraises[1] || '';
  el('finale-praise3').textContent = finalePraises[2] || '';
  var totalDurationMs = computeTotalDurationMs();
  var finaleAvgSec = stats.total > 0 ? (totalDurationMs / stats.total) / 1000 : 0;
  var finalePace = classifyPace(finaleAvgSec);
  el('finale-duration').textContent = formatDuration(totalDurationMs);
  el('finale-time-praise').textContent = pickRandom(timePraisePoolFor(finalePace));
  el('overlay-finale').hidden = false;
  currentOverlayPrimaryAction = function () {
    el('overlay-finale').hidden = true;
    goToResultScreen();
  };
}

export var currentOverlayPrimaryAction = null;

// NG確認オーバーレイの配線は modal.js が持つ（wireNgConfirmOverlay）。
// ここから呼ぶと演出モジュールがモーダルを引いてしまうので、main.js の init() が
// この直後に続けて呼ぶ（配線順は変えていない）。
export function wireOverlays() {
  el('sc-next-btn').addEventListener('click', function () { if (currentOverlayPrimaryAction) currentOverlayPrimaryAction(); });
  el('finale-result-btn').addEventListener('click', function () { if (currentOverlayPrimaryAction) currentOverlayPrimaryAction(); });
}
