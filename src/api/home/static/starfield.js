// トップページの星空背景: 「宇宙空間から見た星空」。
//
// 星は3次元空間の球(半径 FIELD_RADIUS)の中にばらまき、カメラはその内側から
// 透視投影で眺める。星の集まり全体を、カメラの少し後ろにある点(PIVOT)を中心に
// ゆっくり回す。回転中心がカメラ位置と一致していると遠近に関係なく全ての星が同じ
// 角速度で動いて奥行きが出ない。中心をカメラの後ろへずらすと、見えている星は全部
// 同じ向きに流れつつ、遠い星ほど全体の回転(1周 PERIOD_SECONDS)そのままの速さに、
// 手前の星ほどそれより速く動く(=視差)。中心をカメラの前に置くと、中心より奥の星が
// 逆向きに流れて「遠い星ほど動かない」感じが出ないので、後ろに置いている。
// 明るさは距離の2乗に反比例させ、見かけの明るさが一定以上の星にだけ光条を付ける。
// 星は瞬かせない。星雲は星の球のさらに外側に置いた淡い雲の塊で、同じ回転に乗る。
//
// 描画先は #star-canvas。装飾専用なので、失敗しても本文には影響させない。
(function(){
  "use strict";
  var canvas = document.getElementById("star-canvas");
  if(!canvas || !canvas.getContext){ return }
  var ctx = canvas.getContext("2d");
  var motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  var still = motionQuery.matches;

  // ============ 見た目パラメータ(微調整はここだけ) ============
  var CFG = {
    SEED: 20260923,
    PERIOD_SECONDS: 1200,      // 星の集まりが1回転する秒数(20分)
    AXIS: [0.28, 1, 0.18],     // 回転軸(正規化前)。ほぼ縦軸まわり=星は横へ流れる
    FIELD_RADIUS: 1,           // 星を置く球の半径(ワールド単位)
    PIVOT_Z: -0.35,            // 回転中心の位置(z。負=カメラの後ろ)。0だと視差が消える
    NEAR_Z: 0.04,              // これより手前(z)の星は描かない
    NEAR_FADE_Z: 0.16,         // ここから NEAR_Z にかけて手前の星を消していく
    FOV_DEG: 72,               // 画面の対角方向の視野角
    STAR_COUNT: 10000,         // 球全体の星の数(視野に入るのはこの一部)
    REF_DIST: 0.5,             // この距離にある lum=1 の星を「明るさ1」とする
    FLARE_MIN: 1.9,            // 見かけの明るさがこれ以上で光条を付ける
    NEBULA_CLUSTERS: 14,       // 星雲の塊の数(全方向に散らして常にどこかが見えるように)
    NEBULA_PUFFS: 16,          // 塊ひとつあたりの雲の数
    NEBULA_DIST: [1.25, 1.7],  // 星雲を置く距離(回転中心から)
    NEBULA_GAIN: 1.5,          // 星雲の濃さの倍率
    NEBULA_SCALE: 0.25         // 星雲を描く作業用キャンバスの解像度(画面比)。ぼやけた絵なので
                               // 低解像度で描いて拡大しても見た目は変わらず、描画負荷が大きく減る
  };

  // ============ 乱数(固定シード: 毎回同じ空) ============
  function makeRng(seed){
    var state = seed >>> 0;
    return function(){
      state = (state + 0x9E3779B9) >>> 0;
      var z = state;
      z = Math.imul(z ^ (z >>> 16), 0x85EBCA6B);
      z = Math.imul(z ^ (z >>> 13), 0xC2B2AE35);
      return ((z ^ (z >>> 16)) >>> 0) / 4294967296;
    };
  }
  var rng = makeRng(CFG.SEED);
  function between(lo, hi){ return lo + (hi - lo) * rng() }
  function normal(){ // Box-Muller
    var u = 1 - rng(), v = rng();
    return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
  }
  // 単位球面上の一様な方向
  function randomDirection(){
    var z = between(-1, 1), a = between(0, 2 * Math.PI), s = Math.sqrt(1 - z * z);
    return [s * Math.cos(a), s * Math.sin(a), z];
  }

  // ============ 星の色 ============
  // 色温度(ケルビン)から sRGB を求める近似式(Tanner Helland の方法)。
  function kelvinToRgb(kelvin){
    var t = kelvin / 100;
    function clamp(v){ return Math.round(Math.min(255, Math.max(0, v))) }
    var r = t <= 66 ? 255 : 329.698727446 * Math.pow(t - 60, -0.1332047592);
    var g = t <= 66 ? 99.4708025861 * Math.log(t) - 161.1195681661
                    : 288.1221695283 * Math.pow(t - 60, -0.0755148492);
    var b = t >= 66 ? 255 : (t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307);
    return [clamp(r), clamp(g), clamp(b)];
  }
  // [重み, 色温度]。白〜青白を多めに、黄・橙は少なめ。
  var STAR_TYPES = [[0.22, 16000], [0.34, 9500], [0.24, 7000], [0.13, 5600], [0.07, 4200]];
  // [重み, r, g, b]
  var STAR_TINTS = STAR_TYPES.map(function(s){
    var c = kelvinToRgb(s[1]);
    return [s[0], c[0], c[1], c[2]];
  });
  function pickTint(){
    var x = rng(), acc = 0;
    for(var i = 0; i < STAR_TINTS.length; i++){
      acc += STAR_TINTS[i][0];
      if(x <= acc){ return i }
    }
    return STAR_TINTS.length - 1;
  }
  var TINT_CSS = STAR_TINTS.map(function(t){ return "rgb(" + t[1] + "," + t[2] + "," + t[3] + ")" });

  // ============ 星の生成 ============
  // 位置は球内で体積一様(半径は一様乱数の立方根)。つまり遠くの星ほど数が多く、
  // それらは距離のぶん暗く小さく見える。固有の明るさ lum は大半が暗く、ごく一部が明るい。
  var N = CFG.STAR_COUNT;
  var PX0 = new Float32Array(N), PY0 = new Float32Array(N), PZ0 = new Float32Array(N);
  var LUM = new Float32Array(N), TINT = new Uint8Array(N), KEEP = new Float32Array(N);
  (function(){
    for(var i = 0; i < N; i++){
      var d = randomDirection(), r = CFG.FIELD_RADIUS * Math.cbrt(rng());
      // 回転中心まわりの相対位置で持つ(回転はこの相対位置に掛ける)
      PX0[i] = d[0] * r; PY0[i] = d[1] * r; PZ0[i] = d[2] * r;
      var u = rng();
      LUM[i] = 0.18 + 0.5 * u * u + 3.2 * Math.pow(u, 14);
      TINT[i] = pickTint();
      KEEP[i] = rng();
    }
  })();

  // ============ 星雲の生成 ============
  // 塊の中心を全方向にばらし、その周りに大きさの違う雲をガウス分布で散らす。
  // 色は藍・菫・青緑・薄紅の淡いものだけ。
  var NEBULA_TINTS = [[92, 112, 230], [146, 108, 214], [78, 160, 205], [196, 118, 178]];
  var nebula = [];
  (function(){
    for(var c = 0; c < CFG.NEBULA_CLUSTERS; c++){
      var dir = randomDirection();
      var dist = between(CFG.NEBULA_DIST[0], CFG.NEBULA_DIST[1]);
      var tintA = Math.floor(rng() * NEBULA_TINTS.length);
      var tintB = Math.floor(rng() * NEBULA_TINTS.length);
      for(var k = 0; k < CFG.NEBULA_PUFFS; k++){
        var spread = 0.32;
        nebula.push({
          x: dir[0] * dist + normal() * spread,
          y: dir[1] * dist + normal() * spread * 0.6,
          z: dir[2] * dist + normal() * spread,
          size: between(0.25, 0.7),
          alpha: between(0.035, 0.085),
          tint: rng() < 0.65 ? tintA : tintB
        });
      }
    }
  })();

  // ============ スプライト(起動時に1回だけ作る) ============
  function makeCanvas(size){
    var c = document.createElement("canvas");
    c.width = c.height = size;
    return c;
  }
  // 星雲用: 中心がふんわり明るく、縁へなめらかに消える円
  var NEBULA_SPRITES = NEBULA_TINTS.map(function(t){
    var c = makeCanvas(128), g = c.getContext("2d");
    var grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
    var rgb = t[0] + "," + t[1] + "," + t[2];
    grad.addColorStop(0, "rgba(" + rgb + ",1)");
    grad.addColorStop(0.4, "rgba(" + rgb + ",0.5)");
    grad.addColorStop(0.75, "rgba(" + rgb + ",0.14)");
    grad.addColorStop(1, "rgba(" + rgb + ",0)");
    g.fillStyle = grad;
    g.fillRect(0, 0, 128, 128);
    return c;
  });
  // 明るい星用: 小さな光のにじみ + 縦横4本の光条(先へ行くほど細く薄く)
  var FLARE_SIZE = 256, FLARE_HALF = FLARE_SIZE / 2;
  var FLARE_SPRITES = STAR_TINTS.map(function(t){
    var c = makeCanvas(FLARE_SIZE), g = c.getContext("2d");
    var rgb = t[1] + "," + t[2] + "," + t[3];
    g.translate(FLARE_HALF, FLARE_HALF);
    var halo = g.createRadialGradient(0, 0, 0, 0, 0, FLARE_HALF * 0.22);
    halo.addColorStop(0, "rgba(" + rgb + ",0.55)");
    halo.addColorStop(1, "rgba(" + rgb + ",0)");
    g.fillStyle = halo;
    g.fillRect(-FLARE_HALF, -FLARE_HALF, FLARE_SIZE, FLARE_SIZE);
    for(var q = 0; q < 4; q++){
      g.save();
      g.rotate(q * Math.PI / 2);
      var ray = g.createLinearGradient(0, 0, FLARE_HALF, 0);
      ray.addColorStop(0, "rgba(" + rgb + ",0.9)");
      ray.addColorStop(0.35, "rgba(" + rgb + ",0.28)");
      ray.addColorStop(1, "rgba(" + rgb + ",0)");
      g.fillStyle = ray;
      g.beginPath();
      g.moveTo(0, -2.2);
      g.lineTo(FLARE_HALF, 0);
      g.lineTo(0, 2.2);
      g.closePath();
      g.fill();
      g.restore();
    }
    return c;
  });

  // ============ 回転 ============
  // 回転軸 AXIS まわりに角度 angle だけ回す3x3行列(ロドリゲスの回転公式)。
  var axis = (function(){
    var a = CFG.AXIS, l = Math.hypot(a[0], a[1], a[2]);
    return [a[0] / l, a[1] / l, a[2] / l];
  })();
  var M = new Float64Array(9);
  function setRotation(angle){
    var c = Math.cos(angle), s = Math.sin(angle), t = 1 - c;
    var x = axis[0], y = axis[1], z = axis[2];
    M[0] = t*x*x + c;   M[1] = t*x*y - s*z; M[2] = t*x*z + s*y;
    M[3] = t*x*y + s*z; M[4] = t*y*y + c;   M[5] = t*y*z - s*x;
    M[6] = t*x*z - s*y; M[7] = t*y*z + s*x; M[8] = t*z*z + c;
  }

  // ============ 画面 ============
  var width = 0, height = 0, dpr = 1, focal = 1, keepRatio = 1;
  var nebulaCanvas = document.createElement("canvas");
  var nebulaCtx = nebulaCanvas.getContext("2d");
  var angle = 0;
  var omega = 2 * Math.PI / CFG.PERIOD_SECONDS;

  function resize(){
    width = Math.max(1, window.innerWidth);
    height = Math.max(1, window.innerHeight);
    dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = width + "px";
    canvas.style.height = height + "px";
    var halfDiag = Math.hypot(width, height) / 2;
    focal = halfDiag / Math.tan(CFG.FOV_DEG * Math.PI / 360);
    // 狭い画面では同じ視野に同じ数の星が詰まるので、面積に応じて間引く。
    keepRatio = Math.max(0.4, Math.min(1, (width * height) / (1440 * 900)));
    nebulaCanvas.width = Math.max(1, Math.round(width * CFG.NEBULA_SCALE));
    nebulaCanvas.height = Math.max(1, Math.round(height * CFG.NEBULA_SCALE));
    render();
  }

  // ============ 描画 ============
  function render(){
    setRotation(angle);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
    ctx.clearRect(0, 0, width, height);
    var cx = width / 2, cy = height / 2, pz = CFG.PIVOT_Z;
    var m0=M[0],m1=M[1],m2=M[2],m3=M[3],m4=M[4],m5=M[5],m6=M[6],m7=M[7],m8=M[8];

    // --- 星雲(低解像度の作業用キャンバスに加算合成で描き、拡大して貼る) ---
    var ns = CFG.NEBULA_SCALE;
    nebulaCtx.setTransform(1, 0, 0, 1, 0, 0);
    nebulaCtx.globalCompositeOperation = "source-over";
    nebulaCtx.globalAlpha = 1;
    nebulaCtx.clearRect(0, 0, nebulaCanvas.width, nebulaCanvas.height);
    nebulaCtx.setTransform(ns, 0, 0, ns, 0, 0);
    nebulaCtx.globalCompositeOperation = "lighter";
    for(var n = 0; n < nebula.length; n++){
      var b = nebula[n];
      var z = m6*b.x + m7*b.y + m8*b.z + pz;
      if(z < 0.2){ continue }
      var sx = cx + focal * (m0*b.x + m1*b.y + m2*b.z) / z;
      var sy = cy - focal * (m3*b.x + m4*b.y + m5*b.z) / z;
      var size = focal * b.size / z;
      if(sx + size < 0 || sx - size > width || sy + size < 0 || sy - size > height){ continue }
      nebulaCtx.globalAlpha = Math.min(1, b.alpha * CFG.NEBULA_GAIN);
      nebulaCtx.drawImage(NEBULA_SPRITES[b.tint], sx - size, sy - size, size * 2, size * 2);
    }
    ctx.drawImage(nebulaCanvas, 0, 0, width, height);

    // --- 星 ---
    var flares = [];
    var ref2 = CFG.REF_DIST * CFG.REF_DIST;
    var fadeSpan = CFG.NEAR_FADE_Z - CFG.NEAR_Z;
    ctx.globalCompositeOperation = "source-over";
    for(var i = 0; i < N; i++){
      if(KEEP[i] > keepRatio){ continue }
      var x0 = PX0[i], y0 = PY0[i], z0 = PZ0[i];
      var wz = m6*x0 + m7*y0 + m8*z0 + pz;
      if(wz < CFG.NEAR_Z){ continue }
      var wx = m0*x0 + m1*y0 + m2*z0;
      var wy = m3*x0 + m4*y0 + m5*z0;
      var px = cx + focal * wx / wz;
      var py = cy - focal * wy / wz;
      if(px < -8 || px > width + 8 || py < -8 || py > height + 8){ continue }
      var dist2 = wx*wx + wy*wy + wz*wz;
      var bright = Math.min(6, LUM[i] * ref2 / dist2);
      var fade = wz < CFG.NEAR_FADE_Z ? (wz - CFG.NEAR_Z) / fadeSpan : 1;
      var alpha = Math.min(1, 0.24 + 0.8 * Math.sqrt(bright)) * fade;
      if(alpha < 0.03){ continue }
      var radius = Math.min(2.6, 0.5 + 0.8 * Math.sqrt(bright));
      ctx.globalAlpha = alpha;
      ctx.fillStyle = TINT_CSS[TINT[i]];
      if(radius < 1.1){
        ctx.fillRect(px - radius, py - radius, radius * 2, radius * 2);
      }else{
        ctx.beginPath();
        ctx.arc(px, py, radius, 0, 2 * Math.PI);
        ctx.fill();
      }
      if(bright >= CFG.FLARE_MIN){ flares.push(px, py, bright, TINT[i], fade) }
    }

    // --- 明るい星の光条(星の上に加算で重ねる) ---
    ctx.globalCompositeOperation = "lighter";
    for(var f = 0; f < flares.length; f += 5){
      var fb = flares[f + 2];
      var half = Math.min(46, 10 + 12 * Math.sqrt(fb - CFG.FLARE_MIN + 0.1));
      ctx.globalAlpha = Math.min(0.85, 0.35 + 0.15 * fb) * flares[f + 4];
      ctx.drawImage(FLARE_SPRITES[flares[f + 3]], flares[f] - half, flares[f + 1] - half, half * 2, half * 2);
    }
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
  }

  // ============ ループ ============
  var raf = 0, lastTime = 0;
  function tick(now){
    raf = requestAnimationFrame(tick);
    // タブ復帰直後などの大きすぎる経過時間で一気に回らないよう上限を設ける。
    var dt = Math.min(0.05, Math.max(0, (now - lastTime) / 1000));
    lastTime = now;
    angle += omega * dt;
    render();
  }
  function start(){
    if(raf || still){ return }
    lastTime = performance.now();
    raf = requestAnimationFrame(tick);
  }
  function stop(){
    if(raf){ cancelAnimationFrame(raf); raf = 0 }
  }
  document.addEventListener("visibilitychange", function(){
    if(document.hidden){ stop() }else{ start() }
  });
  if(motionQuery.addEventListener){
    motionQuery.addEventListener("change", function(e){
      still = e.matches;
      if(still){ stop(); render() }else{ start() }
    });
  }
  var resizeTimer = 0;
  window.addEventListener("resize", function(){
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(resize, 150);
  }, {passive: true});

  resize();
  start();
})();
