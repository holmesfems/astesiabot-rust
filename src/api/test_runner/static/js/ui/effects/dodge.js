import { P } from '../../constants/i18n.js';
import { NG_DODGE_LIMIT } from '../../constants/config.js';
import { pickRandom } from '../../core/score.js';
import { state, currentItemIsNg } from '../../core/state.js';
import { el, reducedMotionOS } from '../dom.js';

/* =========================================================================
   NGボタン回避（マウスが止まったら逃げる。最大2回/項目）
   ========================================================================= */
export var ngDodgeCount = 0;
var ngDodgeHoverTimer = null;
export var ngDodgeLock = false;
var ngDodgeCooldownUntil = 0;

export function resetNgDodgeState() {
  ngDodgeCount = 0;
  ngDodgeLock = false;
  ngDodgeCooldownUntil = 0;
  clearTimeout(ngDodgeHoverTimer);
  ngDodgeHoverTimer = null;
  var row = el('action-row');
  var okBtn = el('ok-btn');
  var ngBtnEl = el('ng-btn');
  if (row && okBtn && ngBtnEl) {
    // always restore original order: OK left, NG right
    if (okBtn.nextElementSibling !== ngBtnEl) {
      row.insertBefore(okBtn, ngBtnEl);
    }
    okBtn.style.transition = '';
    okBtn.style.transform = '';
    ngBtnEl.style.transition = '';
    ngBtnEl.style.transform = '';
    ngBtnEl.classList.remove('ng-hop');
    var oldBubble = ngBtnEl.querySelector('.ng-bubble');
    if (oldBubble && oldBubble.parentNode) oldBubble.parentNode.removeChild(oldBubble);
  }
}

function ngHoverCapable() {
  return !(window.matchMedia && window.matchMedia('(hover: none)').matches);
}

function showNgDodgeBubble(ngBtnEl) {
  var old = ngBtnEl.querySelector('.ng-bubble');
  if (old && old.parentNode) old.parentNode.removeChild(old);
  var bubble = document.createElement('span');
  bubble.className = 'ng-bubble';
  bubble.textContent = pickRandom(P.NG_DODGE_PHRASES);
  ngBtnEl.appendChild(bubble);
  void bubble.offsetWidth;
  bubble.classList.add('show');
  setTimeout(function () {
    if (bubble.parentNode) bubble.parentNode.removeChild(bubble);
  }, 900);
}

function dodgeNgButton() {
  if (state.transitioning) return;
  if (currentItemIsNg()) return; // 既にNG記録済みの項目では逃げない
  if (ngDodgeLock) return;
  if (ngDodgeCount >= NG_DODGE_LIMIT) return;
  if (Date.now() < ngDodgeCooldownUntil) return;

  var row = el('action-row');
  var okBtn = el('ok-btn');
  var ngBtnEl = el('ng-btn');
  if (!row || !okBtn || !ngBtnEl) return;

  ngDodgeLock = true;
  ngDodgeCount++;

  if (reducedMotionOS) {
    // functional swap only, no motion
    if (okBtn.nextElementSibling === ngBtnEl) {
      row.insertBefore(ngBtnEl, okBtn);
    } else {
      row.insertBefore(okBtn, ngBtnEl);
    }
    showNgDodgeBubble(ngBtnEl);
    ngDodgeCooldownUntil = Date.now() + 500;
    ngDodgeLock = false;
    return;
  }

  var firstOk = okBtn.getBoundingClientRect();
  var firstNg = ngBtnEl.getBoundingClientRect();

  if (okBtn.nextElementSibling === ngBtnEl) {
    row.insertBefore(ngBtnEl, okBtn);
  } else {
    row.insertBefore(okBtn, ngBtnEl);
  }

  var lastOk = okBtn.getBoundingClientRect();
  var lastNg = ngBtnEl.getBoundingClientRect();
  var dxOk = firstOk.left - lastOk.left;
  var dxNg = firstNg.left - lastNg.left;

  okBtn.style.transition = 'none';
  ngBtnEl.style.transition = 'none';
  okBtn.style.transform = 'translateX(' + dxOk + 'px)';
  ngBtnEl.style.transform = 'translateX(' + dxNg + 'px)';
  void row.offsetWidth;
  okBtn.style.transition = 'transform 320ms cubic-bezier(.2,.9,.3,1.2)';
  ngBtnEl.style.transition = 'transform 320ms cubic-bezier(.2,.9,.3,1.2)';
  okBtn.style.transform = '';
  ngBtnEl.style.transform = '';

  ngBtnEl.classList.remove('ng-hop');
  void ngBtnEl.offsetWidth;
  ngBtnEl.classList.add('ng-hop');
  showNgDodgeBubble(ngBtnEl);

  ngDodgeCooldownUntil = Date.now() + 500;
  setTimeout(function () {
    okBtn.style.transition = '';
    ngBtnEl.style.transition = '';
    ngBtnEl.classList.remove('ng-hop');
    ngDodgeLock = false;
  }, 340);
}

var ngLastPointerType = 'mouse';

export function wireNgDodge() {
  var ngBtnEl = el('ng-btn');
  if (!ngBtnEl) return;
  // pointerenter (not the trigger itself) just records whether this hover came from touch,
  // since plain MouseEvents from mouseenter carry no pointerType.
  ngBtnEl.addEventListener('pointerenter', function (e) {
    ngLastPointerType = e.pointerType || 'mouse';
  });
  ngBtnEl.addEventListener('mouseenter', function () {
    if (ngLastPointerType === 'touch') return;
    if (!ngHoverCapable()) return;
    if (state.transitioning) return;
    if (currentItemIsNg()) return; // 既にNG記録済みの項目では逃げない
    if (Date.now() < ngDodgeCooldownUntil) return;
    clearTimeout(ngDodgeHoverTimer);
    ngDodgeHoverTimer = setTimeout(function () {
      dodgeNgButton();
    }, 380);
  });
  ngBtnEl.addEventListener('mouseleave', function () {
    clearTimeout(ngDodgeHoverTimer);
  });
}
