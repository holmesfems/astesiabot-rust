/* ============================================================
   フレームキル計算機 — 表現層（DOM描画・イベント配線・URL共有）

   計算そのものは ./engine.js（DOM非依存）に任せ、ここでは
   カタログ取得(fetch)・状態管理(localStorage保存)・DOM描画・共有URL(#state=...)の
   組み立て/読み込みだけを担当する。日本語専用ページなので文言はここに直書きする
   （lod_chest_solverのような多言語ページではないため、
   strings引数を受け取る設計にはしていない）。

   再描画方針: 構造が変わる操作(行の追加/削除/展開、select/checkbox、
   オペレーター確定)では#app配下を丸ごと作り直す(render。diffしない)。
   入力中の要素がinnerHTML置換で消えるとフォーカスを失うため、どの要素に
   フォーカスがあったかを data-field/data-idx で覚えておいて描画後に復元する
   （withPreservedFocus）。
   一方、数値・テキスト欄への打鍵(input/change)では入力欄そのものは作り直さず、
   そこから導出される表示(判定・折りたたみ行・計算式・↺バッジ)だけを差し替える
   （renderLive）。type="number"はselectionStartがnullでカーソル位置を復元できず、
   作り直すと毎打鍵カーソルが先頭へ戻る（"110"が"011"の順でしか打てない、
   "01"が1に丸められて"100"が打てない）ため。イベントはリスト側の要素に都度張り直すのではなく、
   #app 1箇所に委譲(delegation)することで再描画のたびの張り直しを不要にする。
   ============================================================ */


import { $, showToast } from "./ui/dom.js";
import { onAppChange, onAppClick, onAppInput } from "./ui/events.js";
import { render } from "./ui/page_view.js";
import {
  defaultEnemy,
  fetchCatalog,
  loadSavedState,
  saveState,
  setCatalog,
  setExpandedIdx,
  setGlobalBuffsOpen,
  setState,
  setVerdictSuggestOpen,
  takeStateFromSharedUrl,
} from "./ui/store.js";

export async function initUi() {
  setCatalog(await fetchCatalog());
  const shared = takeStateFromSharedUrl();
  setState(shared ?? loadSavedState() ?? { v: 1, enemy: defaultEnemy(), rows: [], globalBuffIds: [], globalBuffLevels: {}, inspire: { sources: {} } });
  if (shared) {
    showToast("共有URLの内容を読み込みました");
    saveState();
  }
  setExpandedIdx(null);

  const app = $("app");
  app.addEventListener("input", onAppInput);
  app.addEventListener("change", onAppChange);
  app.addEventListener("click", onAppClick);
  // 判定欄(上部sticky)をツール切り替えバーの直下に貼り付けるため、バーの高さを実測して
  // CSS変数に入れる(バーは折り返し無しだがフォント等で高さが変わり得るので決め打ちしない)。
  const syncToolnavHeight = () => {
    const nav = document.querySelector(".toolnav-bar");
    document.documentElement.style.setProperty("--toolnav-h", `${nav ? nav.offsetHeight : 0}px`);
  };
  syncToolnavHeight();
  window.addEventListener("resize", syncToolnavHeight);
  // <details id="global-buffs-details">をユーザーがsummary直クリックで開閉した場合、
  // その状態をrender()後も覚えておく(toggleイベントはbubbleしないためcapture:trueで拾う)。
  app.addEventListener(
    "toggle",
    (ev) => {
      if (ev.target && ev.target.id === "global-buffs-details") setGlobalBuffsOpen(ev.target.open);
      if (ev.target && ev.target.id === "verdict-suggestions") setVerdictSuggestOpen(ev.target.open);
    },
    true,
  );

  render();
}
