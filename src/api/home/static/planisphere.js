// トップページの星空背景: 「星座早見盤」。
//
// 実在の星(Yale Bright Star Catalog。static/stars.json。assets/stars/generate.mjs が作る)を、
// 天の北極を中心にした正距方位図法で描く。早見盤を地上から見上げる向き(天球を内側から見る)なので、
// 赤経は画面上で時計回りに増える(オリオンを北極の下に置くと、肩のベテルギウスが足のリゲルより左)。
// 北極の位置はダイヤル(#dial。金の環と星座記号 #dial-ring を描く)の中心に合わせる。ダイヤルの中心は
// 本文カラム(早見盤のカバー。main.plate)の右辺に固定する: 本物の早見盤のように、カバーが回る盤の
// 半分を隠し、盤はカバーの縁を軸に回る。位置と大きさは layout() が JS で決めて #dial に入れる
// (x = カバーの右辺、y = ツールナビの下の領域の縦の真ん中、直径 = min(78vh, カバーの幅×1.6))。
// その同じ値を北極にも使う(回転中の要素を測り直さない)。ダイヤルは全幅で同じ配置で、スマホは
// カバーが全幅なので中心が画面の右端になり、環の右半分は画面の外にはみ出す(fixed なので横スクロールは出ない)。
// ダイヤルはカバーの背景の上・文字の下に敷く(重ね順は home_index.html の CSS のコメント)。
// 広い画面では金の目印 #dial-index をカバーの右辺の内側に立てる。
// ダイヤルは position:fixed なのでスクロール量の補正は要らない。スクロール量に比例して空(と環)を回す。
// 勝手には動かない: 回るのはスクロールのときだけ。ダイヤルは文字の下で pointer-events:none
// なので掴めない(ホイール・タッチ・キーボードの普通のスクロールに合わせて回るだけ)。
//
// 描画は「1回だけ描いて、あとは CSS で回す」: 空は北極を中心にした一辺 2R の正方形の canvas(#star-canvas。
// 外側を #sky が包む)に回転 0 の姿で1回だけ描き(リサイズのときだけ描き直す)、回転は #sky と #dial-ring の
// CSS の rotate に任せる。昔はスクロールのたびに canvas 全体を描き直していたが、スマホの慣性スクロールは
// コンポジタ側で進むのにメインスレッドの再描画が追いつかず、空と環がガクガクした。いまは回転が
// コンポジタだけで済む。空のグラデーションは北極に対して放射対称なので、一緒に回しても見た目は変わらない
// (星の光条の十字だけは星と一緒に回る。これは許容)。R は「北極から画面の一番遠い角まで」以上にするので、
// 回転する正方形が画面を常に覆う(正方形の外側の隅は縁の色で塗る。body の背景も同じ色)。
//
// 回転の向き: 下へスクロールすると空と環は時計回りに回る(rot = +scrollY * DEG_PER_PX)。
// ダイヤルの中心はカバーの右辺にあり、見えている部分のほとんどは中心より左側にある。指で上へ
// スワイプして内容が上に動くとき、時計回りなら中心の左側は上へ動くので、ダイヤルと空が内容(指)と
// 同じ向きに動いて見える(逆回りだと内容と逆に動いて落ち着かない)。
// 投影は x = px + r*sin(θ), y = py - r*cos(θ)、θ = 赤経(回転0で描く)で、θ を増やすと星は画面上で
// 時計回りに進む(赤経が時計回りに増えるのと同じ)。CSS の rotate も正が時計回りなので、rot をそのまま渡せば星と同じ向きに回る。
//
// 回転の駆動は2通り(どちらも rot = scrollY * DEG_PER_PX):
//   1) スクロール駆動アニメーション(@supports (animation-timeline: scroll()))。home_index.html の CSS が
//      rotate: 0 → var(--rot-max) を scroll(root block) に結び付ける。ここ(JS)は --rot-max =
//      (スクロールできる幅 scrollHeight - clientHeight) × DEG_PER_PX を、リサイズ・フォント読み込み後・
//      内容の高さが変わったとき(ResizeObserver)に入れるだけ。スクロール位置が 0..range のとき 0..rot-max になるので
//      角度は scrollY × DEG_PER_PX と一致する。
//   2) 非対応のブラウザは、スクロールの rAF で #sky と #dial-ring の style.rotate に rot を入れる(canvas は描き直さない)。
// prefers-reduced-motion の人には回さない(静止した星空は描く。CSS 側は media で、JS 側は still で止める)。
//
// オープニング(opening.js。html.opening の間)は、ダイヤルと北極をエンブレム(.opening-stage)の位置・大きさに置く
// (星はエンブレムを中心に見える)。エンブレムの環とダイヤルの環は同じ幾何(viewBox 512 で r 242/232/222/217)なので、
// ステージと同じ大きさのダイヤルがぴったり重なる。この「オープニングの姿」は、本来の位置(layout() が決めた
// left/top/大きさ)からの差を CSS 変数 --open-tx / --open-ty / --open-s に入れ、CSS が html.opening の間だけ
// translate / scale として当てる(FLIP)。opening.js が終わって html.opening-done が付くと、CSS の transition が
// 姿を本来の位置へ戻す(スライド)。空の R はオープニングの姿と本来の姿の両方で画面を覆える大きさにして、1回の描画で足りるようにする。
// 変数だけで姿を作るので、html.opening が外れれば(スキップ・保険のタイマー)自動で本来の姿に戻り、隠れたままにならない。
//
// 天の川は描かない(ぼかした帯は安っぽく見えたため。星と目盛りだけで見せる)。
// 星座の線は、ダイヤルの記号と対になる黄道十二星座だけを淡い金で結ぶ(static/zodiac_lines.json。
// 結び方は generate.mjs に書き起こしたもの)。線分は短いので、両端を投影して直線で結ぶ。
//
// 描画先は #star-canvas。装飾専用なので、失敗しても本文には影響させない。
(function(){
  "use strict";
  try {
    var canvas = document.getElementById("star-canvas");
    var sky = document.getElementById("sky");         // canvas を包む fixed の箱。北極に置いて CSS で回す
    if(!canvas || !canvas.getContext || !sky){ return }
    var root = document.documentElement;
    var ctx = canvas.getContext("2d");
    var motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    var still = motionQuery.matches;

    // ============ 見た目パラメータ(微調整はここだけ) ============
    var CFG = {
      DEG_PER_PX: 0.06,          // スクロール 1px あたりの回転(度)
      DEC_MIN: -55,              // 盤の縁の赤緯(北緯35°の地平線。日本から見える範囲)
      EDGE_SCALE: 1.02,          // 盤の半径を「北極から画面の一番遠い角まで」より少し大きくする倍率
      MAX_DPR: 2,                // 空の canvas の解像度の上限(端末の dpr がこれより高くても)
      MAX_LAYER_PX: 4096,        // 空の canvas の一辺(デバイス px)の上限。大きい画面では dpr を落として収める
      FALLBACK_POLE: [0.75, 0.5],// ダイヤルが見つからないときの北極位置(画面比)
      DIAL_MAX_VH: 0.78,         // ダイヤルの直径の上限(画面の高さ比)
      DIAL_PLATE_RATIO: 1.6,     // ダイヤルの直径の上限(カバーの幅の倍数)。直径 = min(上の高さ比, この幅比)
      NAV_FALLBACK_H: 36,        // ツールナビの高さが測れないときの値(px)
      INDEX_W: 8,                // #dial-index の幅(px。CSS と揃える)
      GRID_DECS: [60, 30, 0, -30],// 赤緯の同心円
      GRID_RA_STEP_H: 2,         // 赤経線の間隔(時)
      GRID_COLOR: "rgba(227,198,123,.10)",
      SKY_STOPS: [[0, "#141c4a"], [0.6, "#0d1336"], [1, "#070a1c"]], // 北極から縁へのグラデーション
      MAG_BASE: 5.6,             // 星の大きさ・濃さの基準等級
      RADIUS_PER_MAG: 0.5,       // 等級が1明るいごとに増える半径(px)
      RADIUS_MIN: 0.45,
      ALPHA_BASE: 0.25,          // 基準等級の星の濃さ
      ALPHA_PER_MAG: 0.18,
      FLARE_MAG: 1.6,            // これより明るい星に十字の光条を付ける
      FLARE_LEN: 5,              // 光条の長さ(星の半径の倍数)
      FLARE_ALPHA: 0.35,
      ZODIAC_COLOR: "rgba(227,198,123,.28)", // 黄道十二星座の線(目盛りより少しだけ濃く、星よりは控えめに)
      ZODIAC_WIDTH: 0.8
    };

    // B-V から星の色(RGB)を選ぶ。
    function colorOf(bv){
      if(bv < 0){ return [200, 215, 255] }
      if(bv < 0.4){ return [235, 238, 255] }
      if(bv < 0.8){ return [255, 246, 228] }
      if(bv < 1.2){ return [255, 226, 186] }
      return [255, 204, 160];
    }

    var DEG = Math.PI / 180;
    var SPAN = 90 - CFG.DEC_MIN;   // 北極から盤の縁までの赤緯の幅(度)

    // ============ 状態 ============
    var width = 0, height = 0, dpr = 1;
    var poleX = 0, poleY = 0, R = 1;
    var openX = 0, openY = 0, opening = false;   // オープニング中のエンブレムの中心(北極の仮の位置)
    var skyGradient = null;
    var ring = document.getElementById("dial-ring");  // 回す環
    var dial = document.getElementById("dial");       // 北極の位置を決めるコンテナ(pointer-events:none)
    var dialIndex = document.getElementById("dial-index");  // カバーの右辺に立てる金の目印(広い画面だけ。狭い画面は CSS で非表示)
    var laidOut = false;      // layout() が中心を決めたか(決めていなければ CSS の位置を測る)
    // スクロール駆動アニメーションが使えるか。home_index.html の @supports と同じ条件にする
    var useCss = !!(window.CSS && CSS.supports && CSS.supports("animation-timeline", "scroll()"));

    // 星(読み込み後に埋める。盤の外の星は読み込み時に捨てる)
    var count = 0;
    var starRa = null, starDec = null, starR = null, starRad = null;
    var starFill = null, starFlare = null;
    var starsLoaded = false;
    // 黄道十二星座の線分([赤経, 赤緯, 赤経, 赤緯] を並べたもの。読み込み後に埋める)
    var zodiac = null;

    // ============ 星の読み込み ============
    function prepareStars(data){
      var n = 0, i;
      for(i = 0; i < data.length; i++){ if(data[i][1] / 10 >= CFG.DEC_MIN){ n++ } }
      count = n;
      starRa = new Float32Array(n); starDec = new Float32Array(n); starR = new Float32Array(n);
      starRad = new Float32Array(n);
      starFill = new Array(n); starFlare = new Array(n);
      var k = 0;
      for(i = 0; i < data.length; i++){
        var s = data[i], dec = s[1] / 10;
        if(dec < CFG.DEC_MIN){ continue }
        var mag = s[2] / 10, c = colorOf(s[3] / 10);
        var rgb = c[0] + "," + c[1] + "," + c[2];
        var bright = CFG.MAG_BASE - mag;
        starRa[k] = s[0] / 10;
        starDec[k] = dec;
        starRad[k] = Math.max(CFG.RADIUS_MIN, bright * CFG.RADIUS_PER_MAG);
        starFill[k] = "rgba(" + rgb + "," + Math.min(1, CFG.ALPHA_BASE + bright * CFG.ALPHA_PER_MAG).toFixed(3) + ")";
        starFlare[k] = mag < CFG.FLARE_MAG ? "rgba(" + rgb + "," + CFG.FLARE_ALPHA + ")" : null;
        k++;
      }
      starsLoaded = true;
      layoutStars();
    }
    // 盤の半径 R が変わったときに、星ごとの北極からの距離を求め直す。
    function layoutStars(){
      if(!starsLoaded){ return }
      for(var i = 0; i < count; i++){ starR[i] = (90 - starDec[i]) / SPAN * R }
    }

    // ============ 画面 ============
    // オープニングの再生中か(opening.js が終わると html.opening-done が付く)。
    function openingNow(){
      return root.classList.contains("opening") && !root.classList.contains("opening-done");
    }
    // ダイヤルの中心をカバーの右辺に置き、直径も決める(理由は冒頭のコメント)。
    // カバーは横には動かない(縦スクロールだけ)ので、読み込み・リサイズ・フォント読み込み後に呼べばよい。
    // 中心の置き方は margin(負の半径)。オープニングの姿を個別の transform プロパティ translate / scale で作るので、
    // transform: translate(-50%,-50%) で中心に合わせると、その -50% まで scale で拡縮されて中心がずれる。
    function layout(){
      var plate = document.querySelector("main.plate");
      if(!dial || !plate){ return }
      var pr = plate.getBoundingClientRect();
      if(pr.width <= 0){ return }
      var bar = document.querySelector(".toolnav-bar");
      var navBottom = bar ? bar.getBoundingClientRect().bottom : CFG.NAV_FALLBACK_H;
      if(!(navBottom > 0)){ navBottom = CFG.NAV_FALLBACK_H }
      var vh = window.innerHeight;
      var cx = pr.right, cy = (navBottom + vh) / 2;
      var size = Math.min(CFG.DIAL_MAX_VH * vh, CFG.DIAL_PLATE_RATIO * pr.width);
      dial.style.setProperty("--dial-size", size + "px");
      dial.style.right = "auto";
      dial.style.left = cx + "px";
      dial.style.top = cy + "px";
      dial.style.margin = (-size / 2) + "px 0 0 " + (-size / 2) + "px";
      if(dialIndex){
        // 三角の先端をカバーの右辺(右の罫線の内側)に付ける。狭い画面は CSS で非表示
        dialIndex.style.left = (cx - 1 - CFG.INDEX_W) + "px";
        dialIndex.style.top = cy + "px";
      }
      poleX = cx; poleY = cy;
      laidOut = true;

      // オープニングの姿: エンブレムの中心に、ステージと同じ直径で置く。本来の姿との差を変数に入れる
      opening = false;
      if(openingNow()){
        var stage = document.querySelector(".opening-stage");
        var sr = stage ? stage.getBoundingClientRect() : null;
        if(sr && sr.width > 0){
          openX = sr.left + sr.width / 2;
          openY = sr.top + sr.height / 2;
          root.style.setProperty("--open-tx", (openX - cx) + "px");
          root.style.setProperty("--open-ty", (openY - cy) + "px");
          root.style.setProperty("--open-s", String(sr.width / size));
          opening = true;
        }
      }
    }
    function measurePole(){
      // layout() が決めた値(poleX/poleY)があればそれをそのまま使う(fixed なのでスクロールの補正は要らない。
      // 環(svg)は中で回るが、コンテナは動かないので中心は変わらない)。無ければ CSS の位置を測る。
      if(laidOut){ return }
      var rect = dial ? dial.getBoundingClientRect() : null;
      if(rect && rect.width > 0){
        poleX = rect.left + rect.width / 2;
        poleY = rect.top + rect.height / 2;
      }else{
        poleX = width * CFG.FALLBACK_POLE[0];
        poleY = height * CFG.FALLBACK_POLE[1];
      }
    }
    // 北極が (px, py) のとき、画面の一番遠い角までの距離。
    function farthest(px, py){
      return Math.max(
        Math.hypot(px, py), Math.hypot(width - px, py),
        Math.hypot(px, height - py), Math.hypot(width - px, height - py));
    }
    function resize(){
      width = Math.max(1, window.innerWidth);
      height = Math.max(1, window.innerHeight);
      layout();
      measurePole();
      // オープニングの姿と本来の姿のどちらでも画面を覆える R。2点の間の途中の姿も、遠い角までの距離が
      // 位置について凸なので両端の大きい方を超えず、覆える
      var far = farthest(poleX, poleY);
      if(opening){ far = Math.max(far, farthest(openX, openY)) }
      R = Math.max(1, far * CFG.EDGE_SCALE);
      // 空は北極を中心にした一辺 2R の正方形。#sky の左上を北極に置いて margin で半分戻し、回転の軸を北極にする
      var side = 2 * R;
      dpr = Math.min(CFG.MAX_DPR, window.devicePixelRatio || 1, CFG.MAX_LAYER_PX / side);
      canvas.width = Math.max(1, Math.round(side * dpr));
      canvas.height = canvas.width;
      canvas.style.width = side + "px";
      canvas.style.height = side + "px";
      sky.style.width = side + "px";
      sky.style.height = side + "px";
      sky.style.left = poleX + "px";
      sky.style.top = poleY + "px";
      sky.style.margin = (-R) + "px 0 0 " + (-R) + "px";
      skyGradient = ctx.createRadialGradient(R, R, 0, R, R, R);
      CFG.SKY_STOPS.forEach(function(s){ skyGradient.addColorStop(s[0], s[1]) });
      layoutStars();
      draw();
      updateRange();
      applyRot();
    }

    // ============ 描画(回転0の姿を1回だけ。回すのは CSS) ============
    // 北極は canvas の中心 (R, R)。
    function draw(){
      var cx = R, cy = R;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.globalAlpha = 1;
      ctx.fillStyle = skyGradient;      // グラデーションは R より外で最後の色が続くので、正方形の隅も縁の色になる
      ctx.fillRect(0, 0, 2 * R, 2 * R);

      // 赤緯の同心円と、2時間ごとの赤経線
      ctx.strokeStyle = CFG.GRID_COLOR;
      ctx.lineWidth = 1;
      var i;
      for(i = 0; i < CFG.GRID_DECS.length; i++){
        ctx.beginPath();
        ctx.arc(cx, cy, (90 - CFG.GRID_DECS[i]) / SPAN * R, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.beginPath();
      for(var h = 0; h < 24; h += CFG.GRID_RA_STEP_H){
        var a = h * 15 * DEG;
        ctx.moveTo(cx, cy);
        ctx.lineTo(cx + R * Math.sin(a), cy - R * Math.cos(a));
      }
      ctx.stroke();

      // 黄道十二星座の線(星より下に描き、星の点が線の上に乗るようにする)
      if(zodiac){
        ctx.strokeStyle = CFG.ZODIAC_COLOR;
        ctx.lineWidth = CFG.ZODIAC_WIDTH;
        ctx.beginPath();
        for(i = 0; i < zodiac.length; i += 4){
          var a1 = zodiac[i] * DEG, r1 = (90 - zodiac[i + 1]) / SPAN * R;
          var a2 = zodiac[i + 2] * DEG, r2 = (90 - zodiac[i + 3]) / SPAN * R;
          ctx.moveTo(cx + r1 * Math.sin(a1), cy - r1 * Math.cos(a1));
          ctx.lineTo(cx + r2 * Math.sin(a2), cy - r2 * Math.cos(a2));
        }
        ctx.stroke();
      }

      if(!starsLoaded){ return }

      // 星。x = px + r sinθ, y = py - r cosθ (θ = 赤経)。回転で画面に入る星が変わるので、画面外でも間引かない
      for(i = 0; i < count; i++){
        var th = starRa[i] * DEG;
        var r = starR[i];
        var x = cx + r * Math.sin(th), y = cy - r * Math.cos(th);
        var rad = starRad[i];
        ctx.fillStyle = starFill[i];
        ctx.beginPath();
        ctx.arc(x, y, rad, 0, Math.PI * 2);
        ctx.fill();
        var fl = starFlare[i];
        if(fl !== null){
          var len = rad * CFG.FLARE_LEN;
          ctx.strokeStyle = fl;
          ctx.lineWidth = 0.7;
          ctx.beginPath();
          ctx.moveTo(x - len, y); ctx.lineTo(x + len, y);
          ctx.moveTo(x, y - len); ctx.lineTo(x, y + len);
          ctx.stroke();
        }
      }
    }

    // ============ 回転 ============
    // CSS のスクロール駆動アニメーション用: スクロールできる幅 × 1px あたりの度 を最大角として入れる。
    // 内容の高さ・画面の高さ(スマホのアドレスバーの出入りも)が変わると幅が変わるので、そのたびに呼ぶ。
    function updateRange(){
      if(!useCss){ return }
      var range = Math.max(0, root.scrollHeight - root.clientHeight);
      root.style.setProperty("--rot-max", (range * CFG.DEG_PER_PX) + "deg");
    }
    // 非対応のブラウザ用: スクロール位置から直接 rotate を入れる(canvas は描き直さない)。
    function applyRot(){
      if(useCss){ return }
      var v = (still ? 0 : window.scrollY * CFG.DEG_PER_PX) + "deg";
      sky.style.rotate = v;
      if(ring){ ring.style.rotate = v }
    }

    // ============ イベント ============
    var pending = false;
    function schedule(){
      if(pending){ return }
      pending = true;
      requestAnimationFrame(function(){ pending = false; applyRot() });
    }
    if(!useCss){ window.addEventListener("scroll", schedule, {passive: true}) }
    if(motionQuery.addEventListener){
      motionQuery.addEventListener("change", function(e){ still = e.matches; schedule() });
    }
    // 内容の高さが変わる(フォント・折り返し・画像)とスクロールできる幅が変わる
    if(useCss && window.ResizeObserver){
      var ro = new ResizeObserver(updateRange);
      ro.observe(document.body);
      var plateEl = document.querySelector("main.plate");
      if(plateEl){ ro.observe(plateEl) }
    }
    // オープニングが終わった(opening.js が dispatch)。スライドは html.opening-done が付いた時点で CSS の
    // transition が勝手に始めるので、ここは後片付けだけ: 姿の変数はもう使われないので外す。
    // 再生中は body が overflow:hidden でスクロールバーが無く、終わって出ると本文の幅が変わる(6vw の余白が
    // 数px動く)ので、クラスが外れたあとにもう一度 resize() して、ダイヤルと北極を本文カバーの右辺に合わせ直す
    // (スキップはすぐ、自然終了はスライド(opening.js の T.HANDOFF)が終わってから)。
    window.addEventListener("home-opening-end", function(e){
      var skipped = !!(e.detail && e.detail.skipped);
      setTimeout(function(){
        root.style.removeProperty("--open-tx");
        root.style.removeProperty("--open-ty");
        root.style.removeProperty("--open-s");
        resize();
      }, skipped ? 0 : 900);
      opening = false;
      updateRange();
      applyRot();
    });

    var resizeTimer = 0;
    window.addEventListener("resize", function(){
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(resize, 150);
    }, {passive: true});

    if(document.fonts && document.fonts.ready){
      document.fonts.ready.then(resize);   // 見出しフォントの読み込みでカバーの位置がずれ得る
    }
    resize();   // 背景とグリッドは星の読み込みを待たずに描く
    fetch("/static/stars.json")
      .then(function(res){ return res.json() })
      .then(function(data){ prepareStars(data); draw() })
      .catch(function(){ /* 星が無くても本文はそのまま使える */ });
    fetch("/static/zodiac_lines.json")
      .then(function(res){ return res.json() })
      .then(function(data){
        zodiac = new Float32Array(data.length * 4);
        for(var j = 0; j < data.length; j++){
          for(var c = 0; c < 4; c++){ zodiac[j * 4 + c] = data[j][c] / 10 }
        }
        draw();
      })
      .catch(function(){ /* 線が無くても星空はそのまま */ });
  } catch(e) { /* 装飾専用。失敗しても本文には影響させない */ }
})();
