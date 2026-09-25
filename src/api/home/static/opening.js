// トップページのオープニング: 星の細剣のエンブレム。
//
// マークアップは templates/home_opening.html（背景なしのオーバーレイ）。再生するかどうかは
// home_index.html の <head> が html.opening クラスで決めており、ここではそのクラスが
// 付いているときだけ動く。流れは
//   星占盤の輪 → 天球儀 → 2本の軌道を星が一周 → 細剣が上から刺さる → 着地の閃光
// で、元の単独版（約5.4秒）から各段の開始を前倒しして重ね、全体を約2秒にしている。
// 再生が終わったら html.opening-done を付けてエンブレムを消し、本文をフェードインさせ、
// 最後にオーバーレイを DOM から外して html.opening を外す（CSS側の遷移は home_index.html）。
// オーバーレイのクリック/タップか任意のキーでスキップできる。
// 装飾専用なので、途中で例外が出ても本文を隠したままにしない（catch で即座に終わらせる）。
(function(){
  "use strict";
  var root = document.documentElement;
  var overlay = document.getElementById("opening");
  if(!root.classList.contains("opening") || !overlay){
    if(overlay) overlay.remove();
    return;
  }

  // ============ タイムライン(ms)。微調整はここだけ ============
  var T = {
    STAR: 0, STAR_STEP: 60, STAR_DUR: 400,          // 背景の小さな星
    RING: 0, RING_STEP: 60, RING_DUR: 600,          // 星占盤の輪
    SPHERE: 180, SPHERE_STEP: 60, SPHERE_DUR: 600,  // 天球儀
    ORB: 350, ORB_DUR: 1000,                        // 軌道（星が一周する）
    SWORD: 1000, SWORD_DUR: 380,                    // 細剣の差し込み（軌道の終わりに重ねる）
    AFTER: 700,                                     // 着地から再生終了まで（閃光の余韻）
    FADE: 450                                       // 本文への切り替え（CSSの transition と揃える）
  };
  var IMPACT = T.SWORD + T.SWORD_DUR;
  var END = IMPACT + T.AFTER;

  var finished = false;
  function finish(){
    if(finished) return;
    finished = true;
    cancelAnimationFrame(raf);
    root.classList.add("opening-done");
    setTimeout(function(){
      overlay.remove();
      root.classList.remove("opening", "opening-done");
    }, T.FADE);
  }

  var raf = 0;
  try {
    var render = build();
    var t0 = 0;
    var frame = function(now){
      if(!t0) t0 = now;
      var t = now - t0;
      render(Math.min(t, END));
      if(t >= END){ finish(); return }
      raf = requestAnimationFrame(frame);
    };
    render(0);
    overlay.classList.add("playing"); // 初期姿を描くまでは CSS で隠している（完成形が一瞬見えないように）
    raf = requestAnimationFrame(frame);
    overlay.addEventListener("click", finish);
    window.addEventListener("keydown", finish, { once: true });
  } catch(e) {
    finish();
  }

  // エンブレムの各パーツにトラックを登録し、時刻 t の姿を描く関数を返す。
  function build(){
    var $ = function(id){ return document.getElementById(id) };
    var clamp = function(v){ return Math.max(0, Math.min(1, v)) };
    var ease = {
      out: function(p){ return 1 - Math.pow(1 - p, 3) },
      inOut: function(p){ return p < .5 ? 4*p*p*p : 1 - Math.pow(-2*p + 2, 3)/2 },
      in: function(p){ return p*p*p },
      back: function(p){ var c = 1.9; return 1 + (c + 1)*Math.pow(p - 1, 3) + c*Math.pow(p - 1, 2) }
    };
    var tracks = [];
    var add = function(start, dur, fn, e){ tracks.push({ start: start, dur: dur, fn: fn, e: e || ease.inOut }) };

    // 線を描く / フェードで出す
    function setupDraw(el, start, dur){
      if(el.hasAttribute("stroke-dasharray")){
        var base = el.getAttribute("opacity") || 1;
        var cx = el.getAttribute("cx") || 256, cy = el.getAttribute("cy") || 256;
        add(start, dur*1.4, function(p){ el.style.opacity = p*base; el.setAttribute("transform", "rotate(" + ((1 - p)*-45) + " " + cx + " " + cy + ")") }, ease.out);
        return;
      }
      var len = 0; try { len = el.getTotalLength() } catch(e) {}
      if(!len){ add(start, dur, function(p){ el.style.opacity = p }); return }
      el.style.strokeDasharray = len;
      var fillOrig = el.getAttribute("fill");
      var fillsSolid = fillOrig && fillOrig !== "none";
      add(start, dur, function(p){
        el.style.strokeDashoffset = len*(1 - p);
        if(fillsSolid && el.id === "") el.style.fillOpacity = clamp((p - .6)/.4);
      });
    }

    // 背景の星
    Array.prototype.forEach.call($("bgstars").querySelectorAll("circle"), function(s, i){
      add(T.STAR + i*T.STAR_STEP, T.STAR_DUR, function(p){ s.style.opacity = p });
    });

    // 星占盤
    Array.prototype.forEach.call($("ring").querySelectorAll("circle"), function(el, i){
      setupDraw(el, T.RING + i*T.RING_STEP, T.RING_DUR);
    });

    // 天球儀（奥と手前の輪を同じタイミングで）
    var sphereEls = Array.prototype.filter.call($("sphere").querySelectorAll("circle,ellipse"), function(el){ return el.getAttribute("stroke") });
    sphereEls.forEach(function(el, i){ setupDraw(el, T.SPHERE + i*T.SPHERE_STEP, T.SPHERE_DUR) });
    Array.prototype.forEach.call($("sphere").querySelectorAll('g[fill="#F1E9D2"] circle'), function(d, i){
      add(T.SPHERE + T.SPHERE_DUR*.8 + i*80, 300, function(p){ d.style.opacity = p });
    });
    Array.prototype.forEach.call($("sphereFront").querySelectorAll("path"), function(el, i){
      setupDraw(el, T.SPHERE + T.SPHERE_STEP*2 + Math.floor(i/2)*T.SPHERE_STEP*1.4, T.SPHERE_DUR);
    });

    // 軌道：星が走った跡に線が引かれ、尻尾は星と一緒に動く
    var NS = "http://www.w3.org/2000/svg";
    var fg = $("orbFront").children; // halo, stroke, tails, stars
    var orb = { rx: 190, ry: 62, cx: 256, cy: 256, lw: 2.5, w1: 10, gap: 0.3, tail: 0.95 };
    function onOrbit(rot, t){
      var r = rot*Math.PI/180, x = orb.rx*Math.cos(t), y = orb.ry*Math.sin(t);
      return [orb.cx + x*Math.cos(r) - y*Math.sin(r), orb.cy + x*Math.sin(r) + y*Math.cos(r)];
    }
    var starEls = fg[3].querySelectorAll("use"), dotEls = fg[3].querySelectorAll("circle");
    function mk(parent, attrs){
      var el = document.createElementNS(NS, "path");
      for(var k in attrs) el.setAttribute(k, attrs[k]);
      parent.appendChild(el);
      return el;
    }
    var orbits = [
      { rot: -30, t0: 0, star: starEls[0], dotT: -Math.PI },
      { rot: 30, t0: Math.PI, star: starEls[1], dotT: 0 }
    ];
    orbits.forEach(function(o){
      var a = o.t0 - orb.gap, b = o.t0 + orb.tail - 2*Math.PI, N = 260;
      o.a = a; o.runs = [];
      var run = null;
      for(var i = 0; i <= N; i++){
        var t = a + (b - a)*i/N, front = Math.sin(t) > 0, pt = onOrbit(o.rot, t);
        if(!run || run.front !== front){
          if(run){ run.pts.push(pt); run.ts.push(t) }
          run = { front: front, pts: [pt], ts: [t] };
          o.runs.push(run);
        } else { run.pts.push(pt); run.ts.push(t) }
      }
      o.runs.forEach(function(r){
        r.cum = [0];
        for(var i = 1; i < r.pts.length; i++) r.cum.push(r.cum[i - 1] + Math.hypot(r.pts[i][0] - r.pts[i - 1][0], r.pts[i][1] - r.pts[i - 1][1]));
        r.len = r.cum[r.cum.length - 1];
        var d = "M" + r.pts.map(function(q){ return q[0].toFixed(1) + "," + q[1].toFixed(1) }).join(" L");
        r.els = r.front
          ? [mk(fg[0], { d: d, "stroke-linecap": "butt" }), mk(fg[1], { d: d, "stroke-linecap": "round" })]
          : [mk($("orbBack").querySelector("g"), { d: d, "stroke-linecap": "round" })];
        r.els.forEach(function(el){ el.style.strokeDasharray = r.len + " " + (r.len + 10) });
      });
      o.tailEl = mk(fg[2], {});
      // 軌道の反対側の点
      o.dot = Array.prototype.find.call(dotEls, function(c){
        var p = onOrbit(o.rot, o.dotT);
        return Math.abs(+c.getAttribute("cx") - p[0]) < 1.5 && Math.abs(+c.getAttribute("cy") - p[1]) < 1.5;
      });
    });
    function tailSeg(o, head, thin, t1, t2){
      var m = Math.max(4, Math.ceil(24*(t2 - t1)/orb.tail)), L = [], R = [];
      for(var i = 0; i <= m; i++){
        var t = t2 + (t1 - t2)*i/m, s = (thin - t)/(thin - head || 1); // s: 細い端0 → 星1
        var p = onOrbit(o.rot, t), p2 = onOrbit(o.rot, t + 1e-4);
        var tx = p2[0] - p[0], ty = p2[1] - p[1], l = Math.hypot(tx, ty) || 1, nx = -ty/l, ny = tx/l;
        var w = (orb.lw + (orb.w1 - orb.lw)*Math.pow(clamp(s), 1.6))/2;
        L.push([p[0] + nx*w, p[1] + ny*w]); R.push([p[0] - nx*w, p[1] - ny*w]);
      }
      return "M" + L.concat(R.reverse()).map(function(q){ return q[0].toFixed(1) + "," + q[1].toFixed(1) }).join(" L") + " Z";
    }
    var isFront = function(t){ return Math.sin(t) > -1e-6 };
    // 尻尾を手前側と奥側に分けて描く（奥側は天球儀と剣に隠れる）
    function tailLayers(o, head, thin){
      var cuts = [head];
      for(var k = Math.ceil(head/Math.PI); k*Math.PI < thin; k++) if(k*Math.PI > head) cuts.push(k*Math.PI);
      cuts.push(thin);
      var f = "", b = "";
      for(var i = 0; i < cuts.length - 1; i++){
        var t1 = cuts[i], t2 = cuts[i + 1];
        if(t2 - t1 < 1e-4) continue;
        var d = tailSeg(o, head, thin, t1, t2);
        if(Math.sin((t1 + t2)/2) > 0) f += d; else b += d;
      }
      return [f, b];
    }
    orbits.forEach(function(o){
      var backG = $("orbBack");
      o.tailBack = document.createElementNS(NS, "path"); o.tailBack.setAttribute("fill", "#E3C67B"); backG.appendChild(o.tailBack);
      o.starBack = o.star.cloneNode(); o.starBack.setAttribute("fill", "#F1E9D2"); backG.appendChild(o.starBack);
    });
    orbits.forEach(function(o){
      add(T.ORB, T.ORB_DUR, function(p){
        var ts = o.a + ((o.t0 - 2*Math.PI) - o.a)*p;   // 星の位置（反時計回りに一周）
        var thin = Math.min(ts + orb.tail, o.a);       // 尻尾の細い端＝線の先端
        o.runs.forEach(function(r){
          var vis = 0;
          if(thin <= r.ts[r.ts.length - 1]) vis = r.len;
          else if(thin < r.ts[0]){
            var i = 0; while(i < r.ts.length - 1 && r.ts[i + 1] > thin) i++;
            var f = (r.ts[i] - thin)/(r.ts[i] - r.ts[i + 1]); vis = r.cum[i] + (r.cum[i + 1] - r.cum[i])*f;
          }
          r.els.forEach(function(el){ el.style.strokeDashoffset = r.len - vis });
        });
        var pos = onOrbit(o.rot, ts);
        var tf = "translate(" + pos[0].toFixed(1) + " " + pos[1].toFixed(1) + ") scale(1.1) rotate(" + ((1 - p)*-360).toFixed(1) + ")";
        var front = p >= 1 || isFront(ts), op = clamp(p*12);
        o.star.setAttribute("transform", tf); o.starBack.setAttribute("transform", tf);
        o.star.style.opacity = front ? op : 0; o.starBack.style.opacity = front ? 0 : op;
        var layers = p > 0 ? tailLayers(o, ts, thin) : ["", ""];
        o.tailEl.setAttribute("d", layers[0]); o.tailBack.setAttribute("d", layers[1]);
        if(o.dot) o.dot.style.opacity = clamp((o.dotT - thin)*6);
      }, function(p){ return 1 - Math.pow(1 - p, 2.2) });
    });

    // 細剣が上から突き刺さる
    var sword = $("sword");
    add(T.SWORD, T.SWORD_DUR, function(p){
      sword.setAttribute("transform", "translate(0 " + (-190*(1 - p)).toFixed(1) + ")");
      sword.style.opacity = clamp(p*2.5);
    }, ease.in);

    // 着地の閃光と衝撃波
    var flash = $("flash"), shock = $("shock"), core = $("core").querySelector("use"), emblem = $("emblem");
    add(IMPACT, T.AFTER, function(p){
      flash.setAttribute("opacity", (p <= 0 ? 0 : p < .12 ? p/.12 : Math.pow(1 - (p - .12)/.88, 1.6)).toFixed(3));
      flash.setAttribute("r", 120 + 140*p);
    }, ease.out);
    add(IMPACT, T.AFTER*.85, function(p){
      shock.setAttribute("r", 20 + 230*p);
      shock.setAttribute("opacity", p > 0 ? (0.85*(1 - p)).toFixed(3) : 0);
      shock.setAttribute("stroke-width", 3*(1 - p) + .3);
    }, ease.out);
    add(IMPACT - 40, 450, function(p){
      core.setAttribute("transform", "translate(256 262) scale(" + (1.2*p).toFixed(3) + ") rotate(" + (1 - p)*90 + ")");
    }, ease.back);
    add(IMPACT, 260, function(p){ emblem.style.transform = "scale(" + (1 + .018*Math.sin(Math.PI*p)) + ")" });

    return function(t){
      for(var i = 0; i < tracks.length; i++){
        var k = tracks[i];
        k.fn(k.e(clamp((t - k.start)/k.dur)));
      }
    };
  }
})();
