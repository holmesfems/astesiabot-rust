// 計算結果（solver.js の CalcResult）を結果カードに描く表示層。数値の整形もここに置く。
// 名前はユーザー入力なので、すべて textContent で入れる（innerHTML に文字列を流さない）。

const RESULT_CARD_IDS = [
  "steps",
  "raw-materials",
  "mined-usage",
  "byproduct-surplus",
  "bottleneck",
  "warnings",
];

const rate = (v) => v.toFixed(2);

function el(tag, text, className) {
  const e = document.createElement(tag);
  if (text !== undefined) e.textContent = text;
  if (className) e.className = className;
  return e;
}

function muted(text) {
  return el("p", text, "muted");
}

// 「名前: 値/min」を <br> 区切りで並べたセル（工程の表の産出・材料・稼働コスト）。
function needsCell(needs) {
  const td = el("td");
  for (const n of needs) {
    td.append(`${n.item}: ${rate(n.rate_per_min)}/min`, el("br"));
  }
  return td;
}

function stepsBody(steps) {
  if (steps.length === 0) return muted("工程はありません。");
  const table = el("table");
  const headRow = el("tr");
  for (const h of [
    "レシピ",
    "装置",
    "台数",
    "律速産出",
    "稼働率",
    "段数",
    "産出(実効/min)",
    "材料消費(/min)",
    "稼働コスト(/min)",
  ]) {
    headRow.append(el("th", h));
  }
  const thead = el("thead");
  thead.append(headRow);
  table.append(thead);
  const tbody = el("tbody");
  for (const s of steps) {
    const tr = el("tr");
    tr.append(
      el("td", s.recipe_name),
      el("td", s.equipment_name),
      el("td", String(s.machine_count)),
      el("td", s.limiting_output),
      el("td", `${(s.utilization * 100).toFixed(1)}%`),
      el("td", String(s.depth)),
      needsCell(s.outputs_effective),
      needsCell(s.inputs_demand),
      needsCell(s.operating_demand)
    );
    tbody.append(tr);
  }
  table.append(tbody);
  return table;
}

// <li>名前: <strong>値</strong>後ろ</li> の一覧。空なら emptyText。
function rateList(needs, emptyText, suffix) {
  if (needs.length === 0) return muted(emptyText);
  const ul = el("ul");
  for (const n of needs) {
    const li = el("li");
    li.append(`${n.item}: `, el("strong", rate(n.rate_per_min)), suffix);
    ul.append(li);
  }
  return ul;
}

function minedUsageBody(usages) {
  if (usages.length === 0) return muted("採掘供給の使用はありません。");
  const ul = el("ul");
  for (const u of usages) {
    const li = el("li");
    li.append(
      `${u.item}: 使用 `,
      el("strong", rate(u.used_rate)),
      `/min ／ 余剰 ${rate(u.surplus_rate)}/min ／ 上限 ${rate(u.cap_rate)}/min`
    );
    ul.append(li);
  }
  return ul;
}

function bottleneckBody(name) {
  if (name === null) return muted("工程はありません。");
  const p = el("p");
  p.append(el("strong", name), " が律速工程です(稼働率が最も高い)。");
  return p;
}

function warningsBody(warnings) {
  const ul = el("ul");
  for (const w of warnings) ul.append(el("li", w));
  return ul;
}

// 各カードの見出し(<h3>)はテンプレートに置き、中身(.card-body)だけ差し替える。
function fill(id, node) {
  const card = document.getElementById(id);
  card.querySelector(".card-body").replaceChildren(node);
  card.hidden = false;
}

export function renderResult(result) {
  document.getElementById("error").hidden = true;
  fill("steps", stepsBody(result.steps));
  fill("raw-materials", rateList(result.raw_materials, "原料の所要はありません。", "/min"));
  fill("mined-usage", minedUsageBody(result.mined_usage));
  fill(
    "byproduct-surplus",
    rateList(result.byproduct_surplus, "副産物の余剰はありません。", "/min(消費されず余る量)")
  );
  fill("bottleneck", bottleneckBody(result.bottleneck));
  fill("warnings", warningsBody(result.warnings));
  document.getElementById("warnings").hidden = result.warnings.length === 0;
}

export function renderError(message) {
  for (const id of RESULT_CARD_IDS) document.getElementById(id).hidden = true;
  const box = document.getElementById("error");
  box.replaceChildren(el("strong", "入力エラー:"), ` ${message}`);
  box.hidden = false;
}
