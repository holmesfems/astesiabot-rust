// フレームキル計算機 — DOM と表示の小道具（整形・トースト・フォーカス保持）と定数。

export const ENEMY_PERCENT_FIELDS = new Set(["defPct", "vulnPct"]);
export const ROW_PERCENT_FIELDS = new Set(["selfPct", "buffPct", "dmgMult"]);

export const BAR_COLORS = ["#4f7cff", "#e8dcb0", "#79e2d0", "#d4794a", "#b48ef0", "#f0748a", "#7bd67b", "#f0a94e"];

/* ---------------- ユーティリティ ---------------- */

export function $(id) {
  return document.getElementById(id);
}

export function fmtInt(n) {
  const r = Math.round(n);
  return r.toLocaleString("ja-JP");
}

export function fmtPct(fraction, digits = 0) {
  const v = fraction * 100;
  return (Math.round(v * 10 ** digits) / 10 ** digits).toString();
}

// 数値inputに詰める値。浮動小数の誤差(0.30000000000000004等)を丸めて見た目を整える。
export function trimNum(n) {
  return Math.round(n * 1e6) / 1e6;
}

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

export function showToast(msg) {
  const container = $("toast-container");
  if (!container) return;
  const el = document.createElement("div");
  el.className = "toast";
  el.textContent = msg;
  container.appendChild(el);
  setTimeout(() => el.classList.add("toast-show"), 10);
  setTimeout(() => {
    el.classList.remove("toast-show");
    setTimeout(() => el.remove(), 300);
  }, 4000);
}

/* ---------------- フォーカス保持付き再描画 ---------------- */

export function withPreservedFocus(fn) {
  const active = document.activeElement;
  // 同じdata-fieldの欄は複数ある(例: 各バフカードのskillLevel/toggleOn)ので、
  // role/idx/buff-id/source-idまで一致する要素にフォーカスを戻す。
  const keys = ["role", "field", "idx", "buffId", "sourceId"];
  const attrs = {};
  if (active && active.dataset && active.dataset.field != null) {
    for (const k of keys) if (active.dataset[k] != null) attrs[k] = active.dataset[k];
  }
  const selStart = active && "selectionStart" in active ? active.selectionStart : null;
  const selEnd = active && "selectionEnd" in active ? active.selectionEnd : null;
  // #appを丸ごと差し替えると一瞬高さが変わってスクロール位置がずれることがあるので戻す。
  const scrollX = window.scrollX;
  const scrollY = window.scrollY;

  fn();

  if (attrs.field != null) {
    const toAttr = (k) => "data-" + k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
    const selector = Object.entries(attrs)
      .map(([k, v]) => `[${toAttr(k)}="${CSS.escape(v)}"]`)
      .join("");
    const el = document.querySelector(selector);
    if (el) {
      // focus()の既定動作はその要素までスクロールするので止める。
      el.focus({ preventScroll: true });
      if (typeof el.setSelectionRange === "function" && selStart != null) {
        try {
          el.setSelectionRange(selStart, selEnd);
        } catch (e) {
          /* select等setSelectionRangeを持たない要素は無視 */
        }
      }
    }
  }
  if (window.scrollX !== scrollX || window.scrollY !== scrollY) window.scrollTo(scrollX, scrollY);
}
