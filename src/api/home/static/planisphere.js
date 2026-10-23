// トップページの星空背景: 「星座早見盤」。
//
// 実在の星(Yale Bright Star Catalog。static/stars.json。assets/stars/generate.mjs が作る)を、
// 天の北極を中心にした正距方位図法で描く。早見盤を地上から見上げる向き(天球を内側から見る)なので、
// 赤経は画面上で時計回りに増える(オリオンを北極の下に置くと、肩のベテルギウスが足のリゲルより左)。
// 北極の位置は右に固定したダイヤル(#dial。金の環 #dial-ring を描く)の中心に合わせ、
// 本文カラム(早見盤のカバー)の後ろから星空が覗くように見せる。ダイヤルは position:fixed なので
// スクロール量の補正は要らない。スクロール量に比例して空(と環)を北極まわりに回す。
// 狭い画面ではダイヤルが右端から覗くつまみになり、北極は画面の右端に来る(空はつまみのまわりを回る)。
// 勝手には動かない: 再描画はスクロールとリサイズのときだけ。
//
// ダイヤルを掴んで回すとページがスクロールする(これもスクロールの別の入口で、ホイール・タッチ・
// キーボードの普通のスクロールはそのまま)。ポインタが北極まわりに角度 turned(度。画面で時計回りが正)
// 回ったら、環と空も同じ向きに turned だけ回って見えてほしい。表示の回転は rot = -scrollY*DEG_PER_PX
// なので、rot を +turned するには scrollY を turned/DEG_PER_PX だけ「減らす」:
// scrollY = 掴んだときの scrollY - turned/DEG_PER_PX。時計回りに回すと上へ戻り、反時計回りに回すと下へ進む
// (下へ進む=反時計回り、と一致する)。ダイヤルだけがポインタを捕まえ、ページの他の場所は普通にスクロールする。
//
// 回転の向き: 時間が進むと星は西へ動き、北を向いた観測者には北極星のまわりを反時計回りに
// 回って見える。投影は x = px + r*sin(θ), y = py - r*cos(θ)、θ = 赤経 + rot で、θ を増やすと
// 星は画面上で時計回りに進む(赤経が時計回りに増えるのと同じ)。反時計回りに回すには θ を
// 減らせばよいので rot = -scrollY * DEG_PER_PX(下へスクロール=時間が進む)。
// rot は負の角度なので、canvas の ctx.rotate と CSS の rotate(どちらも正が時計回り)には
// そのまま渡せば星と同じ向きに回る。
// prefers-reduced-motion の人には rot を 0 に固定する(静止した星空は描く)。
//
// 天の川は描かない(ぼかした帯は安っぽく見えたため。星と目盛りだけで見せる)。
//
// 描画先は #star-canvas。装飾専用なので、失敗しても本文には影響させない。
(function(){
  "use strict";
  try {
    var canvas = document.getElementById("star-canvas");
    if(!canvas || !canvas.getContext){ return }
    var ctx = canvas.getContext("2d");
    var motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    var still = motionQuery.matches;

    // ============ 見た目パラメータ(微調整はここだけ) ============
    var CFG = {
      DEG_PER_PX: 0.06,          // スクロール 1px あたりの回転(度)
      DEC_MIN: -55,              // 盤の縁の赤緯(北緯35°の地平線。日本から見える範囲)
      EDGE_SCALE: 1.02,          // 盤の半径を「北極から画面の一番遠い角まで」より少し大きくする倍率
      FALLBACK_POLE: [0.75, 0.5],// ダイヤルが見つからないときの北極位置(画面比)
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
      FLARE_ALPHA: 0.35
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
    var skyGradient = null;
    // 下のドラッグ処理は読み込み時に dial へリスナーを付けるので、resize() を待たずにここで取る
    var ring = document.getElementById("dial-ring");  // 回す環
    var dial = document.getElementById("dial");       // 北極の位置を決め、掴んで回せるコンテナ

    // 星(読み込み後に埋める。盤の外の星は読み込み時に捨てる)
    var count = 0;
    var starRa = null, starDec = null, starR = null, starRad = null;
    var starFill = null, starFlare = null;
    var starsLoaded = false;

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
    function measurePole(){
      var rect = dial ? dial.getBoundingClientRect() : null;
      if(rect && rect.width > 0){
        // ダイヤルは fixed なのでビューポート座標のまま使える(スクロールの補正は要らない)。
        // 環(svg)は中で回るが、コンテナは動かないので中心は変わらない。
        poleX = rect.left + rect.width / 2;
        poleY = rect.top + rect.height / 2;
      }else{
        poleX = width * CFG.FALLBACK_POLE[0];
        poleY = height * CFG.FALLBACK_POLE[1];
      }
    }
    function resize(){
      width = Math.max(1, window.innerWidth);
      height = Math.max(1, window.innerHeight);
      dpr = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      canvas.style.width = width + "px";
      canvas.style.height = height + "px";
      dial = document.getElementById("dial");
      ring = document.getElementById("dial-ring");
      measurePole();
      var far = Math.max(
        Math.hypot(poleX, poleY), Math.hypot(width - poleX, poleY),
        Math.hypot(poleX, height - poleY), Math.hypot(width - poleX, height - poleY));
      R = Math.max(1, far * CFG.EDGE_SCALE);
      skyGradient = ctx.createRadialGradient(poleX, poleY, 0, poleX, poleY, R);
      CFG.SKY_STOPS.forEach(function(s){ skyGradient.addColorStop(s[0], s[1]) });
      layoutStars();
      draw();
    }

    // ============ 描画 ============
    function draw(){
      var rot = still ? 0 : -window.scrollY * CFG.DEG_PER_PX;  // 度(負=反時計回り)。理由は冒頭のコメント
      if(ring){ ring.style.transform = "rotate(" + rot + "deg)" }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.globalAlpha = 1;
      ctx.fillStyle = skyGradient;
      ctx.fillRect(0, 0, width, height);

      // 赤緯の同心円と、2時間ごとの赤経線
      ctx.strokeStyle = CFG.GRID_COLOR;
      ctx.lineWidth = 1;
      var i;
      for(i = 0; i < CFG.GRID_DECS.length; i++){
        ctx.beginPath();
        ctx.arc(poleX, poleY, (90 - CFG.GRID_DECS[i]) / SPAN * R, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.beginPath();
      for(var h = 0; h < 24; h += CFG.GRID_RA_STEP_H){
        var a = (h * 15 + rot) * DEG;
        ctx.moveTo(poleX, poleY);
        ctx.lineTo(poleX + R * Math.sin(a), poleY - R * Math.cos(a));
      }
      ctx.stroke();

      if(!starsLoaded){ return }

      // 星。x = px + r sinθ, y = py - r cosθ (θ = 赤経 + rot)
      for(i = 0; i < count; i++){
        var th = (starRa[i] + rot) * DEG;
        var r = starR[i];
        var x = poleX + r * Math.sin(th), y = poleY - r * Math.cos(th);
        if(x < -8 || y < -8 || x > width + 8 || y > height + 8){ continue }
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

    // ============ イベント ============
    var pending = false;
    function schedule(){
      if(pending){ return }
      pending = true;
      requestAnimationFrame(function(){ pending = false; draw() });
    }
    window.addEventListener("scroll", schedule, {passive: true});
    if(motionQuery.addEventListener){
      motionQuery.addEventListener("change", function(e){ still = e.matches; schedule() });
    }

    // ダイヤルを掴んで回す → ページをスクロール(向きの理由は冒頭のコメント)。
    // 指の角度は一手ごとに最短の差(-180〜180度)を足していく。180度をまたいでも飛ばないため。
    var dragId = null, lastAngle = 0, turned = 0, startScroll = 0;
    function pointerAngle(e){
      var rect = dial.getBoundingClientRect();
      return Math.atan2(e.clientY - (rect.top + rect.height / 2),
                        e.clientX - (rect.left + rect.width / 2)) / DEG;
    }
    function endDrag(e){
      if(dragId === null || e.pointerId !== dragId){ return }
      try { dial.releasePointerCapture(dragId) } catch(err) { /* 既に外れていてよい */ }
      dragId = null;
      dial.classList.remove("dragging");
    }
    if(dial){
      dial.addEventListener("pointerdown", function(e){
        if(dragId !== null || (e.pointerType === "mouse" && e.button !== 0)){ return }
        dragId = e.pointerId;
        try { dial.setPointerCapture(dragId) } catch(err) { /* 捕まえられなくても動かせる範囲で続ける */ }
        lastAngle = pointerAngle(e);
        turned = 0;
        startScroll = window.scrollY;
        dial.classList.add("dragging");
        e.preventDefault();
      });
      dial.addEventListener("pointermove", function(e){
        if(e.pointerId !== dragId){ return }
        var a = pointerAngle(e);
        var d = a - lastAngle;
        d -= 360 * Math.round(d / 360);
        lastAngle = a;
        turned += d;
        window.scrollTo(0, startScroll - turned / CFG.DEG_PER_PX);  // 再描画はスクロールイベントが行う
      });
      dial.addEventListener("pointerup", endDrag);
      dial.addEventListener("pointercancel", endDrag);
    }

    var resizeTimer = 0;
    window.addEventListener("resize", function(){
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(resize, 150);
    }, {passive: true});

    resize();   // 背景とグリッドは星の読み込みを待たずに描く
    fetch("/static/stars.json")
      .then(function(res){ return res.json() })
      .then(function(data){ prepareStars(data); draw() })
      .catch(function(){ /* 星が無くても本文はそのまま使える */ });
  } catch(e) { /* 装飾専用。失敗しても本文には影響させない */ }
})();
