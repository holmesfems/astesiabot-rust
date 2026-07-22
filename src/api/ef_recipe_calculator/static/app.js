(function () {
  "use strict";

  const BASE = "/EFRecipeCalculator";

  /** @type {{recipes: any[], selected_recipe_ids: string[], raw_items: string[], external_supplies: any[]}} */
  let state = {
    recipes: [],
    selected_recipe_ids: [],
    raw_items: [],
    external_supplies: [],
  };

  let presets = [];

  function uid(prefix) {
    return `${prefix}_${Math.random().toString(36).slice(2, 9)}`;
  }

  function renderAll() {
    renderRecipeList();
    renderRawItems();
    renderExternalSupplies();
    renderTargetItemOptions();
  }

  // ---- レシピエディタ ----

  function buildItemRowsSection(title, list, itemField, numField, onAdd) {
    const wrap = document.createElement("div");
    const h = document.createElement("h4");
    h.textContent = title;
    wrap.appendChild(h);

    list.forEach((row, idx) => {
      const rowEl = document.createElement("div");
      rowEl.className = "item-row";

      const itemInput = document.createElement("input");
      itemInput.type = "text";
      itemInput.placeholder = "アイテム名";
      itemInput.value = row[itemField];
      itemInput.addEventListener("input", () => {
        row[itemField] = itemInput.value;
        renderTargetItemOptions();
      });
      rowEl.appendChild(itemInput);

      const numInput = document.createElement("input");
      numInput.type = "number";
      numInput.step = "0.01";
      numInput.min = "0";
      numInput.value = row[numField];
      numInput.addEventListener("input", () => {
        row[numField] = parseFloat(numInput.value) || 0;
      });
      rowEl.appendChild(numInput);

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "small secondary";
      removeBtn.textContent = "×";
      removeBtn.addEventListener("click", () => {
        list.splice(idx, 1);
        renderRecipeList();
        renderTargetItemOptions();
      });
      rowEl.appendChild(removeBtn);

      wrap.appendChild(rowEl);
    });

    const addBtn = document.createElement("button");
    addBtn.type = "button";
    addBtn.className = "small secondary";
    addBtn.textContent = "+ 行追加";
    addBtn.addEventListener("click", onAdd);
    wrap.appendChild(addBtn);

    return wrap;
  }

  function buildRecipeCard(recipe) {
    const card = document.createElement("div");
    card.className = "recipe-card";

    const head = document.createElement("div");
    head.className = "recipe-card-head";

    const selectLabel = document.createElement("label");
    selectLabel.className = "recipe-select-toggle";
    const selectCheckbox = document.createElement("input");
    selectCheckbox.type = "checkbox";
    selectCheckbox.style.width = "auto";
    selectCheckbox.checked = state.selected_recipe_ids.includes(recipe.id);
    selectCheckbox.addEventListener("change", () => {
      if (selectCheckbox.checked) {
        if (!state.selected_recipe_ids.includes(recipe.id)) {
          state.selected_recipe_ids.push(recipe.id);
        }
      } else {
        state.selected_recipe_ids = state.selected_recipe_ids.filter((id) => id !== recipe.id);
      }
      renderTargetItemOptions();
    });
    selectLabel.appendChild(selectCheckbox);
    selectLabel.append("使用");
    head.appendChild(selectLabel);

    const nameInput = document.createElement("input");
    nameInput.type = "text";
    nameInput.placeholder = "レシピ名";
    nameInput.value = recipe.name;
    nameInput.addEventListener("input", () => {
      recipe.name = nameInput.value;
    });
    head.appendChild(nameInput);

    const equipmentInput = document.createElement("input");
    equipmentInput.type = "text";
    equipmentInput.placeholder = "装置名";
    equipmentInput.value = recipe.equipment_name || "";
    equipmentInput.addEventListener("input", () => {
      recipe.equipment_name = equipmentInput.value;
    });
    head.appendChild(equipmentInput);

    const cycleInput = document.createElement("input");
    cycleInput.type = "number";
    cycleInput.min = "0.1";
    cycleInput.step = "0.1";
    cycleInput.value = recipe.cycle_seconds;
    cycleInput.title = "サイクル秒数";
    cycleInput.addEventListener("input", () => {
      recipe.cycle_seconds = parseFloat(cycleInput.value) || 0;
    });
    head.appendChild(cycleInput);

    const quick = document.createElement("span");
    quick.className = "cycle-quick";
    [2, 10, 20].forEach((sec) => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "small secondary";
      btn.textContent = `${sec}s`;
      btn.addEventListener("click", () => {
        recipe.cycle_seconds = sec;
        cycleInput.value = sec;
      });
      quick.appendChild(btn);
    });
    head.appendChild(quick);

    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "danger small";
    deleteBtn.textContent = "レシピ削除";
    deleteBtn.addEventListener("click", () => {
      state.recipes = state.recipes.filter((r) => r.id !== recipe.id);
      state.selected_recipe_ids = state.selected_recipe_ids.filter((id) => id !== recipe.id);
      renderAll();
    });
    head.appendChild(deleteBtn);

    card.appendChild(head);

    const fields = document.createElement("div");
    fields.className = "field-group";
    fields.appendChild(
      buildItemRowsSection("産出(outputs)", recipe.outputs, "item", "qty", () => {
        recipe.outputs.push({ item: "", qty: 1 });
        renderRecipeList();
        renderTargetItemOptions();
      })
    );
    fields.appendChild(
      buildItemRowsSection("材料(inputs)", recipe.inputs, "item", "qty", () => {
        recipe.inputs.push({ item: "", qty: 1 });
        renderRecipeList();
      })
    );
    fields.appendChild(
      buildItemRowsSection(
        "稼働コスト(operating_costs)",
        recipe.operating_costs,
        "item",
        "rate_per_min",
        () => {
          recipe.operating_costs.push({ item: "", rate_per_min: 0 });
          renderRecipeList();
        }
      )
    );
    card.appendChild(fields);

    return card;
  }

  function renderRecipeList() {
    const container = document.getElementById("recipeList");
    container.innerHTML = "";
    state.recipes.forEach((recipe) => {
      container.appendChild(buildRecipeCard(recipe));
    });
  }

  // ---- 原料の底(raw_items) ----

  function renderRawItems() {
    const container = document.getElementById("rawItemList");
    container.innerHTML = "";
    state.raw_items.forEach((item, idx) => {
      const chip = document.createElement("span");
      chip.className = "chip";

      const input = document.createElement("input");
      input.type = "text";
      input.value = item;
      input.style.width = "140px";
      input.addEventListener("input", () => {
        state.raw_items[idx] = input.value;
      });
      chip.appendChild(input);

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.textContent = "×";
      removeBtn.addEventListener("click", () => {
        state.raw_items.splice(idx, 1);
        renderRawItems();
      });
      chip.appendChild(removeBtn);

      container.appendChild(chip);
    });
  }

  // ---- 採掘供給(external_supplies) ----

  function renderExternalSupplies() {
    const container = document.getElementById("externalSupplyList");
    container.innerHTML = "";
    state.external_supplies.forEach((supply, idx) => {
      const row = document.createElement("div");
      row.className = "item-row";

      const itemInput = document.createElement("input");
      itemInput.type = "text";
      itemInput.placeholder = "アイテム名";
      itemInput.value = supply.item;
      itemInput.addEventListener("input", () => {
        supply.item = itemInput.value;
      });
      row.appendChild(itemInput);

      const rateInput = document.createElement("input");
      rateInput.type = "number";
      rateInput.min = "0";
      rateInput.step = "0.01";
      rateInput.placeholder = "上限/min";
      rateInput.value = supply.max_rate_per_min;
      rateInput.addEventListener("input", () => {
        supply.max_rate_per_min = parseFloat(rateInput.value) || 0;
      });
      row.appendChild(rateInput);

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "small secondary";
      removeBtn.textContent = "削除";
      removeBtn.addEventListener("click", () => {
        state.external_supplies.splice(idx, 1);
        renderExternalSupplies();
      });
      row.appendChild(removeBtn);

      container.appendChild(row);
    });
  }

  // ---- 目標製品セレクト(選択中レシピのoutputsの和集合) ----

  function renderTargetItemOptions() {
    const select = document.getElementById("targetItemSelect");
    const prev = select.value;
    const items = new Set();
    state.recipes.forEach((r) => {
      if (state.selected_recipe_ids.includes(r.id)) {
        r.outputs.forEach((o) => {
          if (o.item) items.add(o.item);
        });
      }
    });
    select.innerHTML = "";
    if (items.size === 0) {
      const opt = document.createElement("option");
      opt.value = "";
      opt.textContent = "(選択中レシピの産出がありません)";
      select.appendChild(opt);
      return;
    }
    items.forEach((item) => {
      const opt = document.createElement("option");
      opt.value = item;
      opt.textContent = item;
      select.appendChild(opt);
    });
    if (items.has(prev)) {
      select.value = prev;
    }
  }

  // ---- ツールバー ----

  document.getElementById("addRecipeBtn").addEventListener("click", () => {
    const recipe = {
      id: uid("recipe"),
      name: "新規レシピ",
      equipment_name: "",
      cycle_seconds: 2,
      outputs: [{ item: "", qty: 1 }],
      inputs: [],
      operating_costs: [],
    };
    state.recipes.push(recipe);
    state.selected_recipe_ids.push(recipe.id);
    renderAll();
  });

  document.getElementById("selectAllBtn").addEventListener("click", () => {
    state.selected_recipe_ids = state.recipes.map((r) => r.id);
    renderAll();
  });

  document.getElementById("addRawItemBtn").addEventListener("click", () => {
    state.raw_items.push("");
    renderRawItems();
  });

  document.getElementById("addExternalSupplyBtn").addEventListener("click", () => {
    state.external_supplies.push({ item: "", max_rate_per_min: 0 });
    renderExternalSupplies();
  });

  // ---- プリセット(1つの入力欄で検索+絞り込み。Discordのコマンド入力風) ----

  let filteredPresets = []; // [{preset, idx}] idxはpresets配列内の元のindex
  let activeSuggestionIndex = -1;
  let selectedPresetIndex = null;

  function renderPresetSuggestions(query) {
    const box = document.getElementById("presetSuggestions");
    const q = (query || "").trim().toLowerCase();
    filteredPresets = presets
      .map((preset, idx) => ({ preset, idx }))
      .filter(({ preset }) => !q || preset.name.toLowerCase().includes(q));
    activeSuggestionIndex = -1;
    box.innerHTML = "";

    if (presets.length === 0) {
      box.hidden = true;
      return;
    }
    if (filteredPresets.length === 0) {
      const empty = document.createElement("div");
      empty.className = "combobox-suggestion-empty";
      empty.textContent = "該当するプリセットがありません";
      box.appendChild(empty);
      box.hidden = false;
      return;
    }
    filteredPresets.forEach(({ preset, idx }) => {
      const item = document.createElement("div");
      item.className = "combobox-suggestion-item";
      item.textContent = preset.name;
      item.addEventListener("mousedown", (e) => {
        e.preventDefault(); // blurより先にこのクリックを確定させる
        choosePreset(idx, preset.name);
      });
      box.appendChild(item);
    });
    box.hidden = false;
  }

  function updateActiveSuggestion() {
    const box = document.getElementById("presetSuggestions");
    const items = box.querySelectorAll(".combobox-suggestion-item");
    items.forEach((el, i) => el.classList.toggle("active", i === activeSuggestionIndex));
    if (items[activeSuggestionIndex]) {
      items[activeSuggestionIndex].scrollIntoView({ block: "nearest" });
    }
  }

  function choosePreset(idx, name) {
    selectedPresetIndex = idx;
    document.getElementById("presetInput").value = name;
    document.getElementById("presetSuggestions").hidden = true;
  }

  fetch(`${BASE}/static/presets.json`)
    .then((res) => res.json())
    .then((data) => {
      presets = data.presets || [];
    })
    .catch(() => {
      // プリセット取得に失敗しても手動編集は継続できるため、握りつぶす。
    });

  const presetInput = document.getElementById("presetInput");
  presetInput.addEventListener("input", () => {
    selectedPresetIndex = null;
    renderPresetSuggestions(presetInput.value);
  });
  presetInput.addEventListener("focus", () => {
    renderPresetSuggestions(presetInput.value);
  });
  presetInput.addEventListener("blur", () => {
    // mousedownでpreventDefault済みなのでクリック確定は先に処理される
    setTimeout(() => {
      document.getElementById("presetSuggestions").hidden = true;
    }, 100);
  });
  presetInput.addEventListener("keydown", (e) => {
    const box = document.getElementById("presetSuggestions");
    if (box.hidden || filteredPresets.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      activeSuggestionIndex = Math.min(activeSuggestionIndex + 1, filteredPresets.length - 1);
      updateActiveSuggestion();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      activeSuggestionIndex = Math.max(activeSuggestionIndex - 1, 0);
      updateActiveSuggestion();
    } else if (e.key === "Enter") {
      if (activeSuggestionIndex >= 0) {
        e.preventDefault();
        const { preset, idx } = filteredPresets[activeSuggestionIndex];
        choosePreset(idx, preset.name);
      }
    } else if (e.key === "Escape") {
      box.hidden = true;
    }
  });

  document.getElementById("loadPresetBtn").addEventListener("click", () => {
    if (selectedPresetIndex === null) return;
    const preset = presets[selectedPresetIndex];
    if (!preset) return;

    state = JSON.parse(JSON.stringify(preset.recipe_set));
    renderAll();
    if (preset.default_target_item) {
      document.getElementById("targetItemSelect").value = preset.default_target_item;
    }
    if (preset.default_target_rate_per_min != null) {
      document.getElementById("targetRateInput").value = preset.default_target_rate_per_min;
    }
  });

  // ---- エクスポート / インポート ----

  document.getElementById("exportBtn").addEventListener("click", () => {
    const blob = new Blob([JSON.stringify(state, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "ef_recipe_set.json";
    a.click();
    URL.revokeObjectURL(url);
  });

  document.getElementById("importFile").addEventListener("change", (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const parsed = JSON.parse(reader.result);
        state = {
          recipes: parsed.recipes || [],
          selected_recipe_ids: parsed.selected_recipe_ids || [],
          raw_items: parsed.raw_items || [],
          external_supplies: parsed.external_supplies || [],
        };
        renderAll();
      } catch (err) {
        alert(`JSON読み込みに失敗しました: ${err}`);
      }
    };
    reader.readAsText(file);
    e.target.value = "";
  });

  // ---- 計算実行 ----

  document.getElementById("calcSubmitBtn").addEventListener("click", () => {
    const targetItem = document.getElementById("targetItemSelect").value;
    const targetRate = parseFloat(document.getElementById("targetRateInput").value);
    const payload = {
      recipe_set: state,
      request: { target_item: targetItem, target_rate_per_min: targetRate },
    };
    document.getElementById("payloadInput").value = JSON.stringify(payload);
    document.getElementById("calcForm").requestSubmit();
  });

  renderAll();
})();
