/* ============================================================
   幽霊船 宝箱ソルバー — 表現層（DOM描画・イベント配線）

   静的ラベルは各言語のページ（templates/lod_index*.html）のmarkupに
   直書きされているので、ここで扱うのは動的に組み立てるテキストだけ。
   その文言は initUi(strings) でページから受け取る。
   言語の切り替えはURL（/LodChestSolver, /LodChestSolver/en）なので、
   実行時に言語を差し替える仕組みは持たない。

   strings に必要なキー:
     hit / miss / open        … 履歴行のラベル（判定ボタンと同じ文言）
     captionTry(n)            … 「N手目」の見出し
     readoutLeft(n)           … 残り候補数（<b>で数字を強調）
     readoutSplit(nh, nm)     … hit/miss それぞれの残り候補数
     worstN(n) / worstAtLeastN(n)
     triesLeft(n)             … 「残り N 回」（回数制限モードのみ表示）
     chanceIn(pct, ok, all)   … 「この回数以内に開く確率 X%（ok/all）」
     trapSprung / trapSprungBody … 回数切れ（罠発動）表示
     altsPrefix               … 「ほぼ同等: 」
     nothingYet / flipResult / deleteTry(n) / poolSummary(n)
     noCandidates / noCandidatesBody
     answerIs(code) / answerBody
     opened / openedBody(n)
   ============================================================ */

import {
  ALL, EXACT_SMALL, EXACT_UNIVERSE,
  bestGuesses, ceilLog2, hintCandidates, narrow,
} from "./engine.js";

export function initUi(s) {
  const $ = id => document.getElementById(id);
  let baseCands = [], cands = [], universeSize = 0, history = [], step = 0,
      useHints = true, limit = 5, limitVal = 5;

  /* --- 設定画面 --- */
  function setMode(hint) {
    useHints = hint;
    $("modeHint").setAttribute("aria-pressed", String(hint));
    $("modeBlind").setAttribute("aria-pressed", String(!hint));
    $("hintRow").hidden = !hint;
    // 総当たりモード（1000通り）は回数を選ばせず、常にS/L前提の無制限モード
    // （最悪手数の最小化）で計算する。6回以下に制限すると1000通りに対しては
    // 成功率がほぼ0になり実用にならないため。
    $("limitFieldset").hidden = !hint;
    validate();
  }
  $("modeHint").onclick = () => setMode(true);
  $("modeBlind").onclick = () => setMode(false);

  const hintInputs = [...$("hintRow").querySelectorAll("input")];
  hintInputs.forEach((el, i) => {
    el.addEventListener("input", () => {
      el.value = el.value.replace(/\D/g, "").slice(0, 1);
      if (el.value && i < 3) hintInputs[i + 1].focus();
      validate();
    });
  });
  function validate() {
    $("startBtn").disabled = useHints && hintInputs.some(el => el.value === "");
  }

  /* --- 回数制限の選択（ヒント既知モードのみ。総当たりモードは無制限固定） --- */
  const limitButtons = [...$("limitRow").querySelectorAll("button")];
  function setLimit(v) {
    limit = v;
    limitButtons.forEach(b => b.setAttribute("aria-pressed", String(Number(b.dataset.limit) === v)));
  }
  limitButtons.forEach(b => b.onclick = () => setLimit(Number(b.dataset.limit)));
  setLimit(5);

  $("startBtn").onclick = () => {
    cands = useHints
      ? hintCandidates(hintInputs.map(el => el.value))
      : ALL.map((_, i) => i);
    baseCands = cands.slice();
    universeSize = cands.length;
    limitVal = useHints ? limit : null;
    history = []; step = 0;
    $("setup").hidden = true;
    $("play").hidden = false;
    $("logBox").hidden = false;
    $("foot").hidden = false;
    render();
  };

  $("resetBtn").onclick = () => {   // 同じ設定のまま1手目に戻す
    history = [];
    refresh();
  };

  $("setupBtn").onclick = () => {   // 幽霊の数字から入れ直す
    $("setup").hidden = false;
    $("play").hidden = true;
    $("result").hidden = true;
    $("logBox").hidden = true;
    $("foot").hidden = true;
    $("log").innerHTML = "";
    $("codeInput").value = "";       // 新しい宝箱では現在のダイヤル状態は無関係
  };

  /* --- 履歴を頭から適用し直す（履歴を編集したとき用） --- */
  function refresh() {
    cands = baseCands.slice();
    let opened = null;
    history.forEach(h => {
      if (h.open) { opened = h; return; }
      cands = narrow(cands, parseInt(h.code, 10), h.hit);
    });
    step = history.length;
    if (opened) {
      drawLog();
      finish("ok", s.opened, s.openedBody(history.indexOf(opened) + 1));
    } else {
      render();
    }
  }

  /* --- メイン描画 --- */
  function render() {
    const n = cands.length;
    drawLog();

    if (n === 0) {
      finish("bad", s.noCandidates, s.noCandidatesBody);
      return;
    }
    const rem = limitVal === null ? null : limitVal - history.filter(h => !h.open).length;
    if (rem !== null && rem <= 0) {          // 回数切れ＝罠発動（暗証番号が振り直される）
      finish("bad", s.trapSprung, s.trapSprungBody);
      return;
    }
    if (n === 1) {
      $("codeInput").value = ALL[cands[0]];   // 前手が残っているとリセット後のpivotが答えの一歩手前になる
      finish("ok", s.answerIs(ALL[cands[0]]), s.answerBody);
      return;
    }

    $("play").hidden = false;
    $("result").hidden = true;
    $("caption").textContent = s.captionTry(step + 1);

    if (limitVal === null) {
      $("tries").hidden = true;
    } else {
      $("tries").hidden = false;
      $("tries").classList.remove("warn");
      $("tries").innerHTML = s.triesLeft(rem);
    }

    const compute = () => {
      const { list, depth, cover } = bestGuesses(cands, universeSize, 3, currentGuess(), rem);
      $("busy").hidden = true;
      const best = list[0];
      $("codeInput").value = ALL[best.guess];
      $("readout").innerHTML =
        s.readoutLeft(n) +
        `<span>${s.readoutSplit(best.nh, best.nm)}</span>` +
        (limitVal === null
          ? (depth !== null ? `<span>${s.worstN(depth)}</span>`
                            : `<span>${s.worstAtLeastN(ceilLog2(n))}</span>`)
          : "");
      if (limitVal !== null && cover !== null) {
        const pct = Math.round(cover / n * 100);
        $("tries").innerHTML = s.triesLeft(rem) + `<span>${s.chanceIn(pct, cover, n)}</span>`;
        $("tries").classList.toggle("warn", cover < n);
      }
      const rest = list.slice(1);
      $("alts").hidden = rest.length === 0;
      if (rest.length) {
        $("alts").innerHTML = s.altsPrefix + rest
          .map(o => `<button type="button" data-c="${ALL[o.guess]}">${ALL[o.guess]}</button>`)
          .join("");
        $("alts").querySelectorAll("button").forEach(b => {
          b.onclick = () => { $("codeInput").value = b.dataset.c; };
        });
      }
      drawPool();
    };

    if (n <= EXACT_SMALL || universeSize <= EXACT_UNIVERSE) {   // 厳密探索は少し時間がかかる
      $("busy").hidden = false;
      $("readout").innerHTML = s.readoutLeft(n);
      setTimeout(compute, 20);
    } else {
      compute();
    }
  }

  function currentGuess() {
    const v = $("codeInput").value.replace(/\D/g, "");
    if (v.length !== 3) return null;
    return parseInt(v, 10);
  }

  function answer(isHit) {
    const g = currentGuess();
    if (g === null) { $("codeInput").focus(); return; }
    history.push({ code: ALL[g], hit: isHit });
    refresh();
  }
  $("btnHit").onclick = () => answer(true);
  $("btnMiss").onclick = () => answer(false);
  $("btnOpen").onclick = () => {
    const g = currentGuess();
    if (g === null) { $("codeInput").focus(); return; }
    history.push({ code: ALL[g], open: true });
    refresh();
  };

  function finish(kind, title, body) {
    $("play").hidden = true;
    const r = $("result");
    r.hidden = false;
    r.innerHTML = `<div class="banner ${kind}"><h2>${title}</h2><p style="margin:0">${body}</p></div>`;
    drawPool();
  }

  function drawLog() {
    const box = $("log");
    if (!history.length) {
      box.innerHTML = '<li><span class="n"></span>'
        + `<span style="color:var(--faint)">${s.nothingYet}</span><span></span><span></span></li>`;
      return;
    }
    box.innerHTML = history.map((h, i) => {
      const res = h.open
        ? `<span class="res">${s.open}</span>`
        : `<button type="button" class="res ${h.hit ? "hit" : "miss"}" data-toggle="${i}"
             title="${s.flipResult}">${h.hit ? s.hit : s.miss}</button>`;
      return `<li><span class="n">${i + 1}</span><span class="code">${h.code}</span>${res}`
        + `<button type="button" class="del" data-del="${i}" aria-label="${s.deleteTry(i + 1)}">×</button></li>`;
    }).join("");
    box.querySelectorAll("[data-toggle]").forEach(b => {
      b.onclick = () => {
        const i = +b.dataset.toggle;
        history[i].hit = !history[i].hit;
        refresh();
      };
    });
    box.querySelectorAll("[data-del]").forEach(b => {
      b.onclick = () => {
        history.splice(+b.dataset.del, 1);
        refresh();
      };
    });
  }

  function drawPool() {
    const box = $("poolBox");
    box.hidden = cands.length === 0;
    box.querySelector("summary").textContent = s.poolSummary(cands.length);
    $("pool").textContent = cands.map(c => ALL[c]).join(" ");
  }

  $("codeInput").addEventListener("input", e => {
    e.target.value = e.target.value.replace(/\D/g, "").slice(0, 3);
  });

  $("year").textContent = new Date().getFullYear();
  validate();
  drawLog();
}
