import { installI18n, S } from './constants/i18n.js';
import { el, showScreen, reducedMotionOS } from './ui/dom.js';
import { wireStartScreen, wireStepScreen, wireResultScreen, handleMaterialCopyClick } from './ui/renderer.js';
import { wireConfirmScreen, wireTermPopup, wireNgConfirmOverlay, pickDefaultTesterName } from './ui/modal.js';
import { initConfetti, wireOverlays } from './ui/effects/confetti.js';
import { checkResumeAvailable, restoreFromHash } from './ui/flow.js';

/* =========================================================================
   初期化
   ========================================================================= */
function init() {
  initConfetti();
  wireStartScreen();
  wireConfirmScreen();
  wireStepScreen();
  wireOverlays();
  wireNgConfirmOverlay(); // 旧 wireOverlays() の末尾で呼ばれていたもの。配線順は変えていない
  wireTermPopup();
  wireResultScreen();
  // 配布物の「リンクをコピー」は確認画面・用語モーダル・ポップアップの3箇所に出るのでまとめて拾う
  document.addEventListener('click', handleMaterialCopyClick);
  checkResumeAvailable();
  showScreen('screen-start');
  el('year').textContent = new Date().getFullYear();
  el('paste-textarea').value = S.SAMPLE_A; // 初見でも「この手順で開始」だけで体験できるようサンプルを既定値に

  // 進捗URL（#state=...）で開かれた場合はここで復元して確認画面かステップ画面へ直行する
  restoreFromHash();
  window.addEventListener('hashchange', function () { restoreFromHash(); });

  // 背景の軽量きらめき粒子を自作
  var sparkleContainer = el('bg-sparkle');
  if (!reducedMotionOS) {
    var n = 24;
    for (var i = 0; i < n; i++) {
      var s = document.createElement('div');
      s.className = 'sparkle';
      s.style.left = (Math.random() * 100) + '%';
      s.style.top = (Math.random() * 100) + '%';
      s.style.animationDelay = (Math.random() * 3.6) + 's';
      sparkleContainer.appendChild(s);
    }
  }
}

export function boot(bundle) {
  installI18n(bundle);
  // 確認モーダルの placeholder に出す既定テスター名。ページロードあたり1回だけ抽選する
  // （installI18n 後でないと P.TESTER_NAME_POOL が埋まっていないため、ここで代入する。
  //   defaultTesterName は ui/modal.js が所有するモジュール変数なので、setter 経由で書き込む）
  pickDefaultTesterName();
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
}
