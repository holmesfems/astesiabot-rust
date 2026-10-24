// 計算結果（optimizer.js の result）を描く表示層。結果のリスト・制御回路の部品の並び・Chart.js のグラフ。
// Chart はページが CDN から読み込む（グローバルの Chart）。

const STATIC = "/WLBatterySimulator/static";

let chartInstance = null;

function el(tag, text, className) {
  const e = document.createElement(tag);
  if (text !== undefined) e.textContent = text;
  if (className) e.className = className;
  return e;
}

function img(name, rotate) {
  const e = document.createElement("img");
  e.src = `${STATIC}/${name}`;
  if (rotate) e.style.transform = "rotate(180deg)";
  return e;
}

// <li>ラベル: <strong>値</strong>後ろ</li>
function item(label, value, suffix = "") {
  const li = el("li");
  li.append(`${label}: `, el("strong", String(value)), suffix);
  return li;
}

// 制御回路の部品の並び。電池の後ろに、ビットが1なら交差器、0なら合流器を置く。
function circuitParts(result) {
  const li = el("li");
  li.style.marginTop = ".4em";
  li.append(img("battery.png"));
  for (const bit of result.bit_str) {
    li.append(bit === "0" ? img("merger.png", result.merger === "right") : img("crosser.png"));
  }
  return li;
}

function destroyChart() {
  if (chartInstance) {
    chartInstance.destroy();
    chartInstance = null;
  }
}

function setChartBody(node) {
  destroyChart();
  document.querySelector("#tvChart .chart-body").replaceChildren(node);
}

function renderChart(result) {
  if (result.remainder === 0) {
    setChartBody(el("p", "最大発電量の倍数で丁度賄えるため、常時フル稼働で運用できます。周期制御は不要です。", "muted"));
    return;
  }
  const box = el("div", undefined, "chartBox");
  const canvas = el("canvas");
  canvas.id = "chart";
  canvas.height = 300;
  box.append(canvas);
  setChartBody(box);

  chartInstance = new Chart(canvas.getContext("2d"), {
    type: "line",
    data: {
      labels: result.time_series,
      datasets: [
        {
          label: "残電量",
          data: result.remaining_series,
          borderWidth: 2,
          tension: 0,
          pointRadius: 0,
        },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      scales: {
        x: {
          type: "linear", // ← ここが重要
          title: { display: true, text: "時間（秒）" },
        },
        y: {
          title: { display: true, text: "残電量" },
          beginAtZero: true,
        },
      },
    },
  });
}

export function renderResult(result) {
  document.getElementById("error").hidden = true;

  const ul = el("ul");
  ul.append(item("要求電力", result.required_power), item("推奨発電量", result.setting_display));
  if (result.remainder !== 0) {
    if (result.clock !== null) ul.append(el("li", `周期設定: ${result.clock}`));
    ul.append(
      circuitParts(result),
      item("残電最小値", result.lowest_storage),
      item("電池節約", result.save_battery, "/日")
    );
  }
  const box = document.getElementById("result");
  box.replaceChildren(el("h3", "計算結果"), ul);
  box.hidden = false;

  renderChart(result);
}

export function renderError(message) {
  document.getElementById("result").hidden = true;
  setChartBody(el("p", "計算後に表示されます。", "muted"));
  const box = document.getElementById("error");
  box.replaceChildren(el("strong", "入力エラー:"), ` ${message}`);
  box.hidden = false;
}
