# エンドフィールド レシピ計算機 — 設計仕様書 (Rust / v2)

> Claude Code への実装指示用ドキュメント。既存 Rust プロジェクトへの機能追加を前提とする。
> この仕様は「何を作るか」と「計算の正しさ」を規定する。具体的な crate 選定・モジュール構成は既存プロジェクトの慣習に従う。

---

## 1. 目的とスコープ

多段生産チェーンにおいて、**最終製品の目標産出レートから、必要な中間素材量・各工程の設備台数・律速工程・副産物や中間素材の余剰を逆算する**計算機。

ゲームのバージョン更新でレシピが頻繁に作り直しになるため、**レシピはコードに直書きせず、ユーザーが動的に入力・編集できる**こと。よく使うレシピはプリセットとして同梱する。

### 今回の実装範囲
- レシピの動的な追加・編集・削除、および**使用レシピの選択**（全選択も可）
- **原料の底をユーザーが指定**（中間素材でもそこで打ち切って原料扱い）
- **採掘供給**（上限レート付き外部ソース）を考慮した控除計算
- **稼働コスト**（材料とは別枠の電力/燃料的コスト。自己消費対応）
- **副産物**（複数産出）の律速判定と余剰報告
- 目標製品＋目標レートからの逆算（設備台数は整数・最小1）

### スコープ外（拡張余地は残す）
- 副産物余剰の自動消費・最適化（レベルB/C）。今回は**レベルA＝余剰は報告のみ**。
- 電力総量チェック、ツリーのビジュアル図、図面コードメモ、代替レシピ選択、永続化/共有URL。

これらは計算エンジンの入出力を変更せずに追加できる構造にすること。

---

## 2. アーキテクチャ方針

計算エンジンを**純粋関数**として UI・IO から完全分離する。エンジンは `RecipeSet` と `CalcRequest` を受け取り `CalcResult` を返すのみ。DOM にもファイル IO にも触れない。テストはこのエンジンに厚く書く。

```
UI 層  ──(RecipeSet, CalcRequest)──▶  計算エンジン(pure)  ──(CalcResult)──▶  UI 層
                                          │
データ層(プリセットJSON / 入力保持)  ◀────┘
```

計算エンジンを独立モジュール（例: `mod solver`）にし、`serde` でシリアライズ可能な純データ型に対して動く関数群として実装する。

### 既存プロジェクト (astesiabot-rust) への準拠

このツールは既存の `WLBatterySimulator`（武陵発電制御計算機）と同じ流儀で実装する。既存スタックとパターンに完全準拠すること:

- **Webフレームワーク**: axum 0.7（`json`, `multipart` feature 済み）
- **テンプレート**: askama 0.12（`#[derive(Template)]` + `templates/*.html`。`{{ x|safe }}` でフラグメント埋め込み）
- **フロント**: htmx 2（CDN）+ **バニラJS**（フレームワークなし。Alpine等は導入しない）。チャートが要る場合のみ Chart.js 4（CDN）。
- **状態**: サーバは**ステートレス**。編集中の RecipeSet はクライアント（バニラJSの配列）が真実の源。
- **送信**: フォーム全体を `hx-post` + `hx-swap="none"` + `multipart/form-data` で送る。可変長ネストのため、RecipeSet を JS で `JSON.stringify` し、**単一 `payload` フィールド**に入れて POST。
- **サーバ復元**: 既存の `while multipart.next_field()` ループで `payload` を拾い、`serde_json::from_str::<RecipeSet>()` で一発復元。serde_json は既存の `preserve_order` feature が効くのでレシピ順序が保たれる。
- **画面更新**: 既存 `render_result_fragment` と同じく、1レスポンスに複数の `hx-swap-oob` 要素を詰めて、工程テーブル/原料/採掘使用/副産物余剰/律速/warning を一括更新。
- **索引・需要マップ**: 標準 HashMap ではなく既存依存の `indexmap::IndexMap` を使い、出力順序を決定的にする（§4 の「決定的にする」要件をこれで満たす）。
- **パス prefix**: `/EFRecipeCalculator`。静的ファイルは `nest_service` で `/EFRecipeCalculator/static`。
- **エンジンとハンドラの関係**: ハンドラは薄いラッパー。既存 `optimizer::optimize(...) -> Result<OptimizationResult, String>` と同じく、`solver::calculate(&RecipeSet, &CalcRequest) -> Result<CalcResult, String>` を呼んで結果をテンプレートに流すだけ。
- **ライセンス/公開**: MIT、GitHub 公開（既存フッターの作法に合わせる）。

---

## 3. データモデル (Rust struct/enum イメージ)

> フィールド名は目安。既存コードの命名規則に合わせてよい。`serde(Serialize, Deserialize)` を全型に付与し、JSON プリセット/エクスポートに対応させる。

### 3.1 アイテムとレート

アイテムは名前 (`String`) で名寄せする。速度は毎分レートを直接持たず、**サイクル秒数 + サイクルあたり個数**で保持し、算出時に毎分へ変換する（端数丸め誤差を避けるため）。

```
rate_per_min = qty as f64 / cycle_seconds as f64 * 60.0
```

サイクルは主に 2s / 10s（イベントで 20s あり）、産出個数は 1〜2 などバラバラ。両形式（サイクル入力・毎分入力）を UI で受けるが、**内部表現は cycle_seconds + qty に正規化**する。毎分入力時は `cycle_seconds` を逆算 or レート保持で算出に用いる（内部正規化推奨）。

### 3.2 型定義

```rust
/// 産出物（主産出・副産物を区別せず outputs に並べる）
struct Output {
    item: String,
    qty: f64,          // 1サイクルあたり産出個数（副産物含む）
}

/// 材料（生産に直接消費される）
struct Input {
    item: String,
    qty: f64,          // 1サイクルあたり消費個数
}

/// 稼働コスト（材料とは別枠。電力/燃料的な、設備を回すための消費）
/// 自己消費（自分が作る物を稼働に食う）もここで表現する。循環扱いしない。
struct OperatingCost {
    item: String,
    rate_per_min: f64, // 設備1台あたりの毎分消費（サイクル非依存。設備が動く限り常時消費）
}

/// 1レシピ = 1設備が回す1種類の生産プロセス
struct Recipe {
    id: String,
    name: String,
    cycle_seconds: f64,          // 1サイクルの秒数
    outputs: Vec<Output>,        // 複数産出（副産物対応）。最低1つ。
    inputs: Vec<Input>,          // 材料
    operating_costs: Vec<OperatingCost>, // 稼働コスト（0個以上）
    // 拡張余地（今回は未使用でよいが Option で持てる形に）:
    // power_draw: Option<f64>,  // 電力チェック用
    // blueprint_code: Option<String>,
    // note: Option<String>,
}

/// 採掘など、上限レートのある外部固定供給
struct ExternalSupply {
    item: String,
    max_rate_per_min: f64,  // 供給上限（例: 息壌ガス 100/min）
}

/// ユーザーが編集する作業セット全体
struct RecipeSet {
    recipes: Vec<Recipe>,               // 定義済みレシピ全部
    selected_recipe_ids: Vec<String>,   // 使用するレシピの選択（全選択も可）
    raw_items: Vec<String>,             // 原料の底として扱うアイテム名の集合。
                                        //   ここに入るアイテムは、産出レシピがあっても展開せず葉にする。
    external_supplies: Vec<ExternalSupply>, // 採掘等の固定供給
}

struct CalcRequest {
    target_item: String,
    target_rate_per_min: f64,
}
```

**索引構築時の注意:**
- アイテム→レシピの索引は `selected_recipe_ids` に含まれるレシピのみで作る。
- 同一アイテムを産出するレシピが選択集合内に複数ある場合は、今回は代替レシピ非対応なのでエラー（warning）に積み、決定的に1つ（例: 先頭）を採用する。将来の代替レシピ対応の余地。
- `raw_items` に含まれるアイテムは、索引に産出レシピがあっても**葉として扱う**（展開停止）。

---

## 4. 計算エンジン仕様

`fn calculate(set: &RecipeSet, req: &CalcRequest) -> CalcResult`

### 4.1 全体の流れ

需要（各アイテムの必要レート /min）を根から展開して集計するが、以下2つが単純 DFS を壊すので反復で解く。

- **稼働コスト**は設備台数が決まらないと需要が確定せず、設備台数は需要が決まらないと確定しない（相互依存）。
- **採掘供給・副産物余剰**は需要を控除する（供給が需要を打ち消す）。

このため「需要マップを反復更新して収束させる」方式を採る。

### 4.2 反復アルゴリズム

```
demand: Map<Item, f64>   // 各アイテムの必要レート /min（正味・控除後）
machines: Map<RecipeId, u64>

初期化:
  demand = { target_item: target_rate_per_min }
  前回 demand のスナップショットを保持

繰り返し (最大 N 回, 例 N=100):
  1. demand を根から再展開する:
     各アイテム it の需要 R について:
       - it が raw_items なら葉。原料所要として記録し、展開しない。
       - it に採掘供給 S があれば、充当 = min(R, S)。残余 R' = max(R - S, 0)。
         （採掘優先・使い切る）。充当・余剰(S-充当)を記録。
       - R' > 0 かつ産出レシピ rec があれば:
           このレシピの「it 産出の毎分/台」 = out_qty(it) / cycle_seconds * 60
           必要台数(浮動) = R' / (it 産出の毎分/台)
         ※ 同一レシピが複数アイテムの需要から要求される場合、
           各要求アイテムについて必要台数(浮動)を出し、その最大値を採る（max 合わせ）。
       - R' > 0 だがレシピも採掘もなく raw でもない → 未定義。warning、原料扱い。
  2. 各選択レシピの必要台数(浮動)の最大要求から、machines = ceil(必要台数(浮動))、最小1。
     （実際に需要>0 で使うレシピのみ台数を持つ。需要0のレシピは0台）
  3. machines が確定したら、各レシピの実効産出/消費を算出:
       - 実効産出(各output) = machines * out_qty / cycle_seconds * 60
       - 材料消費(各input) = machines * in_qty / cycle_seconds * 60
       - 稼働コスト消費 = machines * operating_cost.rate_per_min  （サイクル非依存）
  4. 次期 demand を作り直す:
       - 全レシピの材料消費・稼働コスト消費を、対応アイテムの需要として積み上げる
       - target は常に target_rate_per_min を需要として維持
       - 副産物の余剰供給・採掘供給で控除（4.4）
  5. demand が前回とほぼ一致（全アイテムで差 < ε, 例 1e-6）かつ machines 不変なら収束、break。
  収束せず N 回到達 → warning「収束せず（振動/発散の可能性）」。最後の値を返す。
```

**収束性の注意:** 稼働コストと副産物控除は正/負両方のフィードバックになり得る。通常は数回で収束するが、振動する構成もあり得るので、N 回上限と「差が単調減少しない場合の打ち切り」を入れる。収束しなかった旨は warning で必ず返す（黙って誤答しない）。

### 4.3 副産物の扱い（レベル A）

- レシピは複数 `outputs` を持つ。**どの産出が律速か**は「各産出の需要を満たすのに必要な台数」を出し、その **max** で台数を決める（要件確定）。
- max に合わせた結果、他の産出は供給過多になる。この**余剰 = 実効産出 − その産出への需要充当分**を `byproduct_surplus` として報告する。
- **レベル A なので余剰の自動消費はしない**。余剰は報告のみ。ユーザーが見て手で調整する前提。
  - （将来レベル B: 余剰を採掘供給と同じ「控除される供給源」として demand から差し引くだけで拡張可能。データ構造は既にそれを許す形。）

### 4.4 供給控除の順序（決定的にする）

あるアイテムの需要 R に対する充当順序を固定する（結果の再現性のため）:
1. 採掘供給（external_supplies）を min(R, max_rate) 充当
2. （レベル B 有効時のみ）副産物余剰を充当 ← 今回は無効
3. 残余をレシピ生産で充当

今回は副産物控除を行わないので、副産物余剰は「消費されず余る量」としてそのまま surplus 報告に回る。

### 4.5 出力型

```rust
struct CalcResult {
    steps: Vec<CalcStep>,             // 各レシピ工程（depth 付き）
    raw_materials: Vec<MaterialNeed>, // 原料（葉）の総所要レート
    mined_usage: Vec<MinedUsage>,     // 採掘供給の使用状況（充当/余剰）
    byproduct_surplus: Vec<MaterialNeed>, // 消費されず余る副産物の量
    bottleneck: Option<String>,       // 律速工程（最も稼働率の高い工程）
    warnings: Vec<String>,            // 収束失敗・未定義レシピ・代替レシピ衝突など
}

struct CalcStep {
    recipe_id: String,
    recipe_name: String,
    machine_count: u64,               // 必要設備台数（整数・最小1）
    // 律速となった産出アイテムと、その稼働率
    limiting_output: String,
    utilization: f64,                 // required / effective (0..1)
    outputs_effective: Vec<MaterialNeed>, // 各産出の実効レート
    inputs_demand: Vec<MaterialNeed>,     // 各材料の消費レート
    operating_demand: Vec<MaterialNeed>,  // 稼働コストの消費レート
    depth: u32,
}

struct MaterialNeed { item: String, rate_per_min: f64 }
struct MinedUsage { item: String, used_rate: f64, surplus_rate: f64, cap_rate: f64 }
```

---

## 5. テスト（必須 / 計算エンジンは純粋関数なので厚く）

最低限、以下を単体テストで固定する:

1. **単段**: 原料→最終製品1段。台数・原料レートが正しい。
2. **多段**: 3段以上。中間素材の需要伝播が正しい。
3. **合流**: 複数製品が同一中間素材を要求 → 需要が合算される。
4. **切り上げ**: 端数が出るケースで machine_count が ceil・最小1 になる。
5. **サイクル混在**: 2s/10s/20s・産出1/2個の混在で毎分換算が正しい。
6. **採掘控除**: 採掘上限 < 需要 のとき不足分だけレシピ生産、採掘は使い切り。採掘上限 > 需要 のとき余剰報告、レシピ台数0。
7. **稼働コスト（自己消費）**: 息壌ガス例。「息壌1→息壌ガス1、稼働に息壌ガス6/min」。設備台数増に伴う稼働コスト需要が反復で正しく足し戻され、収束する。循環参照エラーにならないこと。
8. **反復収束**: 稼働コスト連鎖で数回反復して収束。収束値が理論値と一致。
9. **収束失敗**: 意図的に振動/発散する構成で、N 回打ち切り＋warning が出る（誤答を黙って返さない）。
10. **副産物 max 合わせ**: 銅例（銅鉱石+水→銅塊+汚水）。
    - 下流が銅塊のみ要求 → 銅塊需要で台数決定、汚水は全量 surplus。
    - 下流が汚水のみ要求 → 汚水需要で台数決定、銅塊が surplus。
    - 両方要求 → 各需要の必要台数の max で決定、少ないほうが surplus。
11. **原料の底指定**: raw_items に息壌を入れると、息壌の産出レシピがあっても展開停止し、息壌が原料として所要レート報告される。
12. **レシピ選択**: selected_recipe_ids に含まれないレシピは索引に入らない。未選択で経路が切れる場合は未定義 warning。

---

## 6. UI 仕様

### 6.1 レシピ入力（動的）
- レシピの追加/編集/削除。各レシピで:
  - 名前 / サイクル秒数（2s・10s・20s のクイック選択があると良い）
  - outputs: アイテム名＋産出個数の行を可変個（副産物はここに足す）
  - inputs: 材料アイテム名＋消費個数の行を可変個
  - operating_costs: アイテム名＋毎分消費の行を可変個（電力/燃料/自己消費）
  - 速度は「サイクル形式」「毎分形式」両対応（内部は cycle_seconds+qty に正規化）

### 6.2 レシピ選択 / 原料指定 / 採掘供給
- 使用レシピの選択 UI（チェックボックス等）。「全選択」ボタンあり。
- 原料の底に置くアイテムの指定 UI（raw_items 編集）。
- 採掘供給テーブルの編集（アイテム名＋上限/min）。

### 6.3 計算と結果
- 最終製品（選択レシピの outputs から選択）＋目標レート /min を入力 → 計算。
- 結果表示:
  - 工程テーブル（レシピ名 / 台数 / 律速産出 / 稼働率 / 各産出実効 / 各材料消費 / 稼働コスト消費 / 段数）
  - 原料の総所要レート
  - 採掘供給の使用状況（充当 / 余剰 / 上限）
  - **副産物の余剰**（消費されず余る量）
  - 律速工程
  - warning（収束失敗・未定義・代替レシピ衝突）

### 6.4 配布・永続化
- 既存プロジェクトの構成に従う。RecipeSet の serde によるエクスポート/インポート関数を用意（将来の保存/共有の土台）。ブラウザストレージ等の永続化は今回スコープ外。

### 6.5 実装構成（astesiabot-rust 準拠 / 具体）

#### ルーティング（axum 0.7、既存 `router()` パターン踏襲）
```rust
pub fn router<S>() -> Router<S>
where S: Clone + Send + Sync + 'static {
    Router::new()
        .route("/", get(index))
        .route("/calculate", get(calculate_redirect).post(calculate))
        // 任意: プリセット取得を別エンドポイントにするなら
        .route("/presets", get(presets))
        .nest_service("/static", ServeDir::new(STATIC_DIR))
}
// 上位で `.nest("/EFRecipeCalculator", router())` する想定
```

#### ハンドラ（既存 `calculate(Multipart)` パターン踏襲。payload を JSON で受ける）
```rust
async fn calculate(mut multipart: Multipart) -> Html<String> {
    let mut payload: Option<String> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        let Some(name) = field.name().map(|s| s.to_string()) else { continue };
        let Ok(text) = field.text().await else { continue };
        if name == "payload" { payload = Some(text); }
    }
    let Some(payload) = payload else {
        return render_error_fragment("入力がありません。".into());
    };
    // RecipeSet + CalcRequest をまとめた入力型を JSON で受ける
    let input: CalcInput = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(e) => return render_error_fragment(format!("JSON解析に失敗: {e}")),
    };
    match solver::calculate(&input.recipe_set, &input.request) {
        Ok(result) => render_result_fragment(result),
        Err(error) => render_error_fragment(error),
    }
}
```
`CalcInput { recipe_set: RecipeSet, request: CalcRequest }` はフォーム送信用のまとめ型。

#### OOB フラグメント（既存 `render_result_fragment` 踏襲。複数領域を1レスポンスで）
成功時レスポンスは以下の OOB 要素を連結して返す（`hx-swap-oob="outerHTML"`）:
- `#error`（hidden にして消す）
- `#steps`（工程テーブル）
- `#raw-materials`（原料所要）
- `#mined-usage`（採掘使用状況）
- `#byproduct-surplus`（副産物余剰）
- `#bottleneck`（律速）
- `#warnings`（warning 一覧。空なら hidden）

askama テンプレートは領域ごとに分割（`steps.html`, `raw_materials.html`, … / `error.html`）し、`#[derive(Template)]` 型を用意。既存の ErrorTemplate/ResultTemplate と同じ流儀。

#### フロント（index.html、htmx 2 + バニラJS）
- フォームは既存同様 `hx-post="/EFRecipeCalculator/calculate"` `hx-swap="none"` `hx-encoding="multipart/form-data"`。
- **RecipeSet はバニラJSの配列で保持**（既存の blueprintMap をオブジェクトで持つのと同じ発想）。レシピ行・材料行・産出行・稼働コスト行・採掘供給の追加/削除/編集は JS が配列を更新して DOM を再描画。
- 計算時、hidden input `name="payload"` に `JSON.stringify({recipe_set, request})` を書き込んでから `calcForm.requestSubmit()`（既存が図面切替時にやっているのと同じ手法）。
- 結果は htmx が OOB で各領域に差し込む。チャート等を後で足す場合のみ、既存の `htmx:afterSettle` フックパターンを流用。

#### 保存/読み込み（ステートレスの弱点を埋める）
- 「エクスポート」: 現在の JS 配列（RecipeSet）を `JSON.stringify` して Blob 化しダウンロード。
- 「インポート」: ファイル読み込み→`JSON.parse`→JS 配列に反映→DOM 再描画。
- サーバの serde 経路（`serde_json`）と JSON スキーマが同一なので、POST 送信・保存・プリセット読込が全部同じ形。


---

## 7. 同梱プリセット（プレースホルダ / ダミー数値）

> ⚠️ 数値はすべて構造説明用のダミー。**実際のゲーム内数値はあなたがゲーム内で確認して差し替える**こと。計算結果の信頼性は数値の正確さに直結する。

```json
{
  "presets": [
    {
      "name": "銅ライン（副産物あり・例）",
      "recipes": [
        {
          "id": "cu1",
          "name": "銅塊生産",
          "cycle_seconds": 2,
          "outputs": [
            { "item": "銅塊", "qty": 1 },
            { "item": "汚水", "qty": 1 }
          ],
          "inputs": [
            { "item": "銅鉱石", "qty": 1 },
            { "item": "水", "qty": 1 }
          ],
          "operating_costs": []
        }
      ]
    },
    {
      "name": "息壌ガス（自己消費あり・例）",
      "recipes": [
        {
          "id": "gas1",
          "name": "息壌ガス化",
          "cycle_seconds": 2,
          "outputs": [ { "item": "息壌ガス", "qty": 1 } ],
          "inputs": [ { "item": "息壌", "qty": 1 } ],
          "operating_costs": [ { "item": "息壌ガス", "rate_per_min": 6 } ]
        }
      ]
    }
  ]
}
```

---

## 8. 実装チェックリスト（Claude Code 向け）

### 計算エンジン（solver モジュール / 純粋関数）
- [ ] 型定義（Output/Input/OperatingCost/Recipe/ExternalSupply/RecipeSet/CalcRequest/CalcResult/CalcStep/MaterialNeed/MinedUsage）に serde 付与
- [ ] レート正規化（cycle_seconds+qty ⇔ per_min の相互変換ユーティリティ）
- [ ] 索引構築は `indexmap::IndexMap`（選択レシピのみ / raw_items 停止 / 代替レシピ衝突 warning / 出力順序を決定的に）
- [ ] 反復ソルバ（需要展開→台数 ceil・最小1→実効算出→需要再構築→収束判定, 上限N・ε）
- [ ] 採掘控除（採掘優先・使い切り・充当/余剰記録）
- [ ] 稼働コストの反復足し戻し（自己消費を循環扱いしない）
- [ ] 副産物 max 合わせ台数決定＋余剰報告（レベルA）
- [ ] 律速（稼働率最大）判定
- [ ] 収束失敗時 warning（黙って誤答しない）
- [ ] `solver::calculate(&RecipeSet, &CalcRequest) -> Result<CalcResult, String>` として公開
- [ ] 単体テスト §5 の 1〜12 全件

### Web層（axum 0.7 + askama 0.12、既存 WLBatterySimulator 準拠）
- [ ] `router()` に `/`(index) と `/calculate`(post) と `/static`(nest_service)。上位で `/EFRecipeCalculator` にネスト
- [ ] `calculate` ハンドラ: multipart から `payload` を拾い `serde_json::from_str::<CalcInput>()` で復元 → solver 呼び出し
- [ ] askama テンプレート: index.html + 領域別フラグメント（steps / raw_materials / mined_usage / byproduct_surplus / bottleneck / warnings）+ error.html、各々 `#[derive(Template)]`
- [ ] 成功/エラーとも複数 `hx-swap-oob="outerHTML"` を連結して返す（既存 render_result_fragment 踏襲）

### フロント（htmx 2 + バニラJS、CDN。新規ライブラリ追加なし）
- [ ] フォーム: `hx-post=/EFRecipeCalculator/calculate` `hx-swap="none"` `hx-encoding="multipart/form-data"`
- [ ] RecipeSet をバニラJS配列で保持。レシピ/材料/産出/稼働コスト/採掘の行を動的に追加・削除・編集し DOM 再描画
- [ ] 使用レシピ選択（全選択ボタン）/ 原料の底指定 / 採掘供給テーブルの UI
- [ ] 計算時: hidden `name="payload"` に `JSON.stringify({recipe_set, request})` を入れ `requestSubmit()`（既存の図面切替パターン流用）
- [ ] JSON エクスポート（Blobダウンロード）/ インポート（ファイル→parse→配列反映→再描画）
- [ ] プリセット読み込み UI（同梱JSON → 配列へ流し込み）

### 仕上げ
- [ ] プリセットのダミー数値を実データに差し替え（開発者作業）
- [ ] MIT ライセンス表記・GitHub リンク（既存フッター流儀）

## 付録: 拡張ポイント（今回作らない）
- レベルB（副産物余剰の自動充当）: §4.4 の順序2を有効化し、副産物余剰を demand から控除するだけ。データ構造は対応済み。
- 電力チェック: Recipe.power_draw を足し、machines*power_draw を後処理集計。
- ツリー可視化: CalcStep.depth＋親子（parent_item を足す余地）から描画層で再構成。
- 代替レシピ: アイテム→Recipe を アイテム→Vec<Recipe>＋選択 に拡張。
- **カプセル化（複数レシピの畳み込み / プロトタイプ後に検討）**: サブチェーンの計算結果を1個の `Recipe`（合成 inputs/outputs/operating_costs）に変換して表現する。**ソルバから見ればカプセルも通常の Recipe と同一に扱える**ため、コア計算エンジンは変更不要。実装時は「カプセルも Recipe 型で表現できる」性質を壊さないこと。カプセル境界での採掘供給・副産物余剰・自己消費の扱い（内部で閉じるか外に露出するか）は、プロトタイプを触ってから要件を確定する。