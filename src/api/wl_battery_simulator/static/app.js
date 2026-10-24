import { optimize, parseIntegerInput } from "./optimizer.js";
import { renderResult, renderError } from "./render.js";

const codeValue = document.getElementById("codeValue");
const inputFreq = document.getElementById("input_freq");
const copyBtn = document.getElementById("copyBtn");
const toast = document.getElementById("toast");
const selector = document.getElementById("blueprintSelector");
const blueprintImg = document.getElementById("blueprintImg");
const tutorial = document.getElementById("tutorial_link");
const calcForm = document.getElementById("calcForm");
const requiredPowerInput = document.getElementById("required_power");

let toastTimer;

function showToast(message = "コピー完了しました") {
  toast.textContent = message;
  toast.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.classList.remove("show"), 1400);
}

// 図面ID -> 表示内容 のマップ
const blueprintMap = {
  CTL_1: {
    code: "EFO01iUIAE8OIE29i72U4",
    inputFreq: "1.5/s",
    imgSrc: "/WLBatterySimulator/static/blueprint.png",
    imgAlt: "発電制御器 設計図（入力1.5/min）",
    tutorial: null,
  },
  CTL_3: {
    code: "EFO01iUIAE8OIE2u272U4",
    inputFreq: "30/s 倉庫入力",
    imgSrc: "/WLBatterySimulator/static/blueprint3.png",
    imgAlt: "発電制御器 設計図（可変周期）",
    tutorial: "https://youtu.be/Wq1ki2xgoEQ",
  },
  CTL_4: {
    code: "EFO012O9Eo1I9o819eIOo",
    inputFreq: "30/s 倉庫入力",
    imgSrc: "/WLBatterySimulator/static/blueprint3.png",
    imgAlt: "発電制御器 設計図（可変周期）",
    tutorial: "https://youtu.be/Wq1ki2xgoEQ",
  },
};

function applySelection(itemId) {
  const item = blueprintMap[itemId];
  if (!item) return;

  codeValue.textContent = item.code;
  inputFreq.textContent = item.inputFreq;
  blueprintImg.src = item.imgSrc;
  blueprintImg.alt = item.imgAlt;
  blueprintImg.dataset.itemId = itemId;

  // チュートリアルリンクの表示制御
  if (item.tutorial) {
    const a = document.createElement("a");
    a.href = item.tutorial;
    a.classList.add("muted");
    a.textContent = "チュートリアルを見る";
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    tutorial.replaceChildren(a);
    tutorial.hidden = false;
  } else {
    tutorial.hidden = true;
    tutorial.replaceChildren();
  }
}

// ---- 計算(サーバーには送らず、ブラウザだけで計算する) ----

function calculate() {
  const data = new FormData(calcForm);
  const requiredPower = parseIntegerInput(data.get("required_power"));
  const storageMargin = parseIntegerInput(data.get("storage_margin"));
  if (requiredPower === null || storageMargin === null) {
    renderError("入力値が不正です。");
    return;
  }
  const { result, error } = optimize(
    requiredPower,
    storageMargin,
    data.get("use_margin_under_5") !== null,
    selector.value
  );
  if (error) {
    renderError(error);
  } else {
    renderResult(result);
  }
}

// submit で計算する（ブラウザの入力チェック required/min/step と Enter での計算をそのまま使う）。
calcForm.addEventListener("submit", (e) => {
  e.preventDefault();
  calculate();
});

selector.addEventListener("change", (e) => {
  applySelection(e.target.value);
  // 必要電力が入力済みなら、図面切替と同時に再計算する
  if (requiredPowerInput.value) calcForm.requestSubmit();
});

// 初期反映（ページ表示時にselectの値から反映）
applySelection(selector.value);

copyBtn.addEventListener("click", async () => {
  await navigator.clipboard.writeText(codeValue.textContent.trim());
  showToast("コピー完了しました");
});
