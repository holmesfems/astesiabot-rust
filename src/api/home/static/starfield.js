// トップページの星空背景。天球(単位球)上に星と天の川を置き、ステレオ投影で
// 描画する。視線は天の北極まわりに一定速度で回り続ける(=空がゆっくり日周運動する)。
// 星の配置はシード付き乱数なので毎回同じ空になる。
// 描画先は #star-canvas。装飾専用なので失敗しても本文には影響させない。
(function(){
  "use strict";
  var canvas = document.getElementById("star-canvas");
  if(!canvas || !canvas.getContext){ return }
  var ctx = canvas.getContext("2d");
  var reduceMQ = window.matchMedia("(prefers-reduced-motion: reduce)");
  var reduced = reduceMQ.matches;
  var TAU = Math.PI * 2, D2R = Math.PI / 180;

  // ============ 見た目パラメータ(微調整はここだけ) ============
  var SKY = {
    PERIOD_SECONDS: 600,   // 天球が1周する秒数(10分)
    VIEW_DEC_DEG: 52,      // 視線の赤緯。北極が画面上端のすぐ外に来る程度
    VIEW_RA0_DEG: 300,     // 初期の視線の赤経(はくちょう座付近=天の川が画面を横切る)
    ROLL_DEG: 24,          // 画面の傾き。正で北極が右上へ寄る
    FOV_DEG: 60,           // 対角方向の視野角(狭い画面では FOV_MOBILE_DEG)
    FOV_MOBILE_DEG: 46,
    STARS: 4300,           // 全天の星の数(うち STARS_FIELD 個は一様分布、残りは銀河面に集中)
    STARS_FIELD: 2900,
    MW_CLOUDS: 2300,       // 天の川の光の雲(ぼかしスプライト)
    MW_DUST: 3600,         // 天の川の微光星(1pxの点)
    MW_GAIN: 1.7,          // 天の川の明るさ倍率(参考実装比。本文の裏でも見えるよう強め)
    METEOR_FIRST_MS: 5000, // 最初の流れ星までの時間
    METEOR_GAP_MS: [9000, 23000]
  };
  var OMEGA = TAU / SKY.PERIOD_SECONDS;

  // ============ シード付き乱数 ============
  function mulberry32(a){return function(){a|=0;a=a+0x6D2B79F5|0;var t=Math.imul(a^a>>>15,1|a);t=t+Math.imul(t^t>>>7,61|t)^t;return((t^t>>>14)>>>0)/4294967296;};}
  var rnd = mulberry32(90417);
  function gauss(){var u=0;while(u===0)u=rnd();return Math.sqrt(-2*Math.log(u))*Math.cos(TAU*rnd());}
  function radec(ra,dec){var c=Math.cos(dec);return [c*Math.cos(ra),c*Math.sin(ra),Math.sin(dec)];}
  function norm(v){var l=Math.hypot(v[0],v[1],v[2])||1;return [v[0]/l,v[1]/l,v[2]/l];}
  function dot(a,b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
  function cross(a,b){return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];}

  // ============ 銀河座標系(J2000の銀河北極と銀河中心) ============
  // 天の川を実際の空と同じ位置・傾きに流すため。
  var GN = radec(192.8595*D2R, 27.1283*D2R);
  var GA = radec(266.405*D2R, -28.936*D2R);
  (function(){var d=dot(GA,GN);GA=norm([GA[0]-d*GN[0],GA[1]-d*GN[1],GA[2]-d*GN[2]]);})();
  var GB = cross(GN, GA);
  function galactic(l,b){var cb=Math.cos(b),sb=Math.sin(b),cl=Math.cos(l),sl=Math.sin(l);
    return [cb*(cl*GA[0]+sl*GB[0])+sb*GN[0], cb*(cl*GA[1]+sl*GB[1])+sb*GN[1], cb*(cl*GA[2]+sl*GB[2])+sb*GN[2]];}

  // ============ 星 ============
  // 等級は暗い星ほど多い指数分布。色は色温度(青白→白→黄→橙)のグラデーションから取る。
  var NS = SKY.STARS;
  var SX=new Float32Array(NS),SY=new Float32Array(NS),SZ=new Float32Array(NS);
  var MAG=new Float32Array(NS),RAD=new Float32Array(NS),ALP=new Float32Array(NS);
  var PH=new Float32Array(NS),FQ=new Float32Array(NS),TB=new Uint8Array(NS);
  var SEL=new Float32Array(NS); // 狭い画面での間引き用の一様乱数(resize の density と比べる)
  var COL=new Array(NS);
  var TSTOPS=[[0,[157,184,255]],[.3,[206,220,255]],[.5,[255,248,236]],[.72,[255,226,173]],[1,[255,179,102]]];
  function tcol(t){for(var k=1;k<TSTOPS.length;k++){if(t<=TSTOPS[k][0]){var a=TSTOPS[k-1],b=TSTOPS[k],f=(t-a[0])/(b[0]-a[0]);return [0,1,2].map(function(j){return Math.round(a[1][j]+(b[1][j]-a[1][j])*f)});}}return TSTOPS[4][1];}
  for(var i=0;i<NS;i++){
    var v;
    if(i<SKY.STARS_FIELD){var z=2*rnd()-1,ra=TAU*rnd(),c=Math.sqrt(1-z*z);v=[c*Math.cos(ra),c*Math.sin(ra),z];}
    else{v=galactic(rnd()*TAU, gauss()*0.14);}
    SX[i]=v[0];SY[i]=v[1];SZ[i]=v[2];
    var m=Math.max(-1.3, 6.6+Math.log10(rnd()+1e-9)/0.44);
    MAG[i]=m;
    RAD[i]=0.55+Math.max(0,6.6-m)*0.42;
    ALP[i]=Math.min(1,0.34+(6.6-m)*0.17);
    PH[i]=rnd()*TAU; FQ[i]=0.0012+rnd()*0.0035;
    var t=Math.min(1,Math.max(0,0.5+gauss()*0.21));
    var col=tcol(t); COL[i]="rgb("+col[0]+","+col[1]+","+col[2]+")";
    TB[i]=Math.min(4,Math.floor(t*5));
    SEL[i]=rnd();
  }

  // ============ 天の川 ============
  // 銀河面に沿って、中心方向ほど厚く明るい雲を置く。中心付近には暗黒帯(ダストレーン)を
  // 抜き、低周波のノイズで濃淡のむらを付ける。
  var NMW=SKY.MW_CLOUDS, NDU=SKY.MW_DUST;
  var MX=new Float32Array(NMW),MY=new Float32Array(NMW),MZ=new Float32Array(NMW),MS=new Float32Array(NMW),MA=new Float32Array(NMW),MT=new Uint8Array(NMW);
  var DX=new Float32Array(NDU),DY=new Float32Array(NDU),DZ=new Float32Array(NDU),DA=new Float32Array(NDU);
  function mwSample(thick){
    for(var guard=0;guard<60;guard++){
      var l = rnd()<0.52 ? rnd()*TAU-Math.PI : gauss()*0.95;
      var cf=(1+Math.cos(l))/2;
      var b=gauss()*(thick+0.07*cf*cf);
      var lane=0.014*Math.sin(l*2.3)+0.004;
      if(Math.abs(l)<1.3 && Math.abs(b-lane)<0.026*(0.55+cf) && rnd()<0.82) continue;
      var nz=0.5+0.5*Math.sin(l*11.7+Math.sin(l*3.1)*2)*Math.cos(b*37+l*5.3);
      if(rnd()>0.3+0.7*nz) continue;
      return {v:galactic(l,b),cf:cf};
    }
    return {v:galactic(rnd()*TAU,0),cf:0.3};
  }
  for(i=0;i<NMW;i++){
    var s=mwSample(0.075);
    MX[i]=s.v[0];MY[i]=s.v[1];MZ[i]=s.v[2];
    MS[i]=(12+rnd()*34)*(0.8+0.5*s.cf);
    MA[i]=(0.028+0.05*rnd())*(0.5+0.65*s.cf);
    var r=rnd(); MT[i]= r<0.2?2:(r<0.55?0:1);
  }
  for(i=0;i<NDU;i++){
    var s2=mwSample(0.055);
    DX[i]=s2.v[0];DY[i]=s2.v[1];DZ[i]=s2.v[2];
    DA[i]=0.12+rnd()*0.4*(0.5+s2.cf);
  }

  // ============ スプライト(放射グラデーションの円) ============
  function blob(r,g,b,a){var c=document.createElement("canvas");c.width=c.height=64;var x=c.getContext("2d");var gr=x.createRadialGradient(32,32,0,32,32,32);gr.addColorStop(0,"rgba("+r+","+g+","+b+","+a+")");gr.addColorStop(.35,"rgba("+r+","+g+","+b+","+(a*.45)+")");gr.addColorStop(1,"rgba("+r+","+g+","+b+",0)");x.fillStyle=gr;x.fillRect(0,0,64,64);return c;}
  var MWSPR=[blob(150,176,255,1),blob(255,240,216,1),blob(255,196,150,1)];
  var GLOW=[];for(var k=0;k<5;k++){var gc=tcol((k+.5)/5);GLOW.push(blob(gc[0],gc[1],gc[2],.9));}

  // ============ 投影 ============
  // 天球を視線の赤経 ra だけZ軸まわりに回し、赤緯 dec だけ傾けてから、視線方向を
  // 中心にステレオ投影する。最後に画面内でロール(ROLL_DEG)を掛ける。
  // 回転は ra を増やすだけ=天の北極まわりの剛体回転なので、星は北極を中心に弧を描く。
  var viewRa = SKY.VIEW_RA0_DEG*D2R;
  var viewDec = SKY.VIEW_DEC_DEG*D2R;
  var cR=1,sR=0,cD=Math.cos(viewDec),sD=Math.sin(viewDec);
  var cRoll=Math.cos(SKY.ROLL_DEG*D2R), sRoll=Math.sin(SKY.ROLL_DEG*D2R);
  var SC=600,CX=0,CY=0,W=0,H=0,DPR=1,SZSCALE=1;
  var starDensity=1, drawCloudsN=NMW, drawDustN=NDU;
  var PX=0,PY=0,PD=0;
  function setView(){cR=Math.cos(viewRa);sR=Math.sin(viewRa);}
  function proj(x,y,z){
    var x1=x*cR+y*sR, y1=-x*sR+y*cR;
    var d=x1*cD+z*sD;
    if(d<-0.3) return false;
    var up=-x1*sD+z*cD;
    var k=SC*2/(1+d);
    var ux=-y1*k, uy=-up*k;
    PX=CX+ux*cRoll-uy*sRoll; PY=CY+ux*sRoll+uy*cRoll; PD=d;
    return true;
  }
  function onScreen(m){return PX>-m&&PX<W+m&&PY>-m&&PY<H+m;}

  function resize(){
    W=Math.max(1,window.innerWidth);H=Math.max(1,window.innerHeight);
    DPR=Math.min(2,window.devicePixelRatio||1);
    canvas.width=Math.round(W*DPR);canvas.height=Math.round(H*DPR);
    canvas.style.width=W+"px";canvas.style.height=H+"px";
    var mobile=W<760;
    CX=W/2; CY=H/2;
    var halfDiag=Math.hypot(W,H)/2;
    var fov=(mobile?SKY.FOV_MOBILE_DEG:SKY.FOV_DEG)*D2R;
    SC=halfDiag/(2*Math.tan(fov/2));
    SZSCALE=Math.max(0.55,Math.min(1.4,SC/700));
    // 狭い画面では同じ視野に同じ数の星が詰まって重く・うるさくなるので、面積に応じて
    // 間引く。星は配列の前半が一様分布・後半が銀河面なので先頭から切ると銀河面だけが
    // 消える。星ごとの一様乱数 SEL で選ぶ。天の川の雲と微光星は生成順に偏りが無いので
    // 先頭から一部だけ描けばよい。
    var density=Math.max(0.45,Math.min(1,(W*H)/(1440*900)));
    starDensity=density;
    drawCloudsN=Math.round(NMW*Math.max(0.7,density));
    drawDustN=Math.round(NDU*density);
    if(reduced){ draw(performance.now()) }
  }

  // ============ 描画 ============
  var meteor=null, nextMeteor=performance.now()+SKY.METEOR_FIRST_MS;

  function draw(now){
    setView();
    ctx.setTransform(DPR,0,0,DPR,0,0);
    ctx.globalCompositeOperation="source-over";ctx.globalAlpha=1;
    ctx.clearRect(0,0,W,H);

    // 天の川(加算合成で重なるほど明るく)
    ctx.globalCompositeOperation="lighter";
    for(var i=0;i<drawCloudsN;i++){
      if(!proj(MX[i],MY[i],MZ[i])||PD<-0.1) continue;
      var s=MS[i]*SZSCALE*(1.2/(0.2+PD*0.5+0.5));
      if(!onScreen(s)) continue;
      ctx.globalAlpha=Math.min(1,MA[i]*SKY.MW_GAIN);
      ctx.drawImage(MWSPR[MT[i]],PX-s/2,PY-s/2,s,s);
    }
    ctx.fillStyle="#e3e8ff";
    for(i=0;i<drawDustN;i++){
      if(!proj(DX[i],DY[i],DZ[i])||!onScreen(2)) continue;
      ctx.globalAlpha=Math.min(1,DA[i]*0.85*SKY.MW_GAIN);ctx.fillRect(PX,PY,1,1);
    }
    ctx.globalCompositeOperation="source-over";

    drawStars(now);
    drawMeteor(now);
    ctx.globalAlpha=1;
  }

  // 明るい星(等級<3.4)は色付きのグロー、特に明るい星(<1.2)は十字の光条を付ける。
  function drawStars(now){
    var amp=reduced?0:0.32;
    for(var i=0;i<NS;i++){
      if(SEL[i]>starDensity||!proj(SX[i],SY[i],SZ[i])||!onScreen(30)) continue;
      var x=PX,y=PY;
      var tw=1-amp*(0.5+0.5*Math.sin(now*FQ[i]+PH[i]))*(MAG[i]>3?1:0.55);
      var r=RAD[i]*(0.85+0.3*SZSCALE);
      var a=ALP[i]*tw;
      if(MAG[i]<3.4){
        var gs=r*(MAG[i]<1.5?11:8);
        ctx.globalAlpha=a*0.55;ctx.drawImage(GLOW[TB[i]],x-gs/2,y-gs/2,gs,gs);
      }
      ctx.globalAlpha=a;ctx.fillStyle=COL[i];
      if(r<1.1){ctx.fillRect(x-r,y-r,r*2,r*2);}
      else{ctx.beginPath();ctx.arc(x,y,r,0,TAU);ctx.fill();}
      if(MAG[i]<1.2){
        var L=r*6*tw;ctx.globalAlpha=0.35*tw;ctx.strokeStyle=COL[i];ctx.lineWidth=0.7;
        ctx.beginPath();ctx.moveTo(x-L,y);ctx.lineTo(x+L,y);ctx.moveTo(x,y-L);ctx.lineTo(x,y+L);ctx.stroke();
      }
    }
  }

  function drawMeteor(now){
    if(reduced)return;
    if(!meteor&&now>nextMeteor){
      meteor={x:W*(0.1+Math.random()*0.6),y:H*(0.08+Math.random()*0.4),a:Math.PI*0.15+Math.random()*0.35,t0:now,len:120+Math.random()*120,sp:0.9+Math.random()*0.5};
      nextMeteor=now+SKY.METEOR_GAP_MS[0]+Math.random()*(SKY.METEOR_GAP_MS[1]-SKY.METEOR_GAP_MS[0]);
    }
    if(!meteor)return;
    var t=(now-meteor.t0)/900;if(t>1){meteor=null;return;}
    var d=t*meteor.len*3*meteor.sp;
    var hx=meteor.x+Math.cos(meteor.a)*d,hy=meteor.y+Math.sin(meteor.a)*d;
    var tx=hx-Math.cos(meteor.a)*meteor.len*(1-t*0.5),ty=hy-Math.sin(meteor.a)*meteor.len*(1-t*0.5);
    var g=ctx.createLinearGradient(tx,ty,hx,hy);var fa=Math.sin(t*Math.PI);
    g.addColorStop(0,"rgba(255,245,220,0)");g.addColorStop(1,"rgba(255,245,220,"+0.8*fa+")");
    ctx.globalAlpha=1;ctx.strokeStyle=g;ctx.lineWidth=1.2;
    ctx.beginPath();ctx.moveTo(tx,ty);ctx.lineTo(hx,hy);ctx.stroke();
  }

  // ============ ループ ============
  var raf=0,lastT=0;
  function frame(now){
    raf=requestAnimationFrame(frame);
    // タブ復帰直後などの大きすぎるdtで一気に回らないようクランプする。
    var dt=Math.min(0.05,Math.max(0,(now-lastT)/1000));lastT=now;
    viewRa+=OMEGA*dt;
    draw(now);
  }
  function start(){if(!raf&&!reduced){lastT=performance.now();raf=requestAnimationFrame(frame);}}
  function stop(){if(raf){cancelAnimationFrame(raf);raf=0;}}
  document.addEventListener("visibilitychange",function(){ if(document.hidden){stop()}else{start()} });
  if(reduceMQ.addEventListener){
    reduceMQ.addEventListener("change",function(e){
      reduced=e.matches;
      if(reduced){ stop(); draw(performance.now()) } else { start() }
    });
  }
  var rzT=0;
  window.addEventListener("resize",function(){ clearTimeout(rzT); rzT=setTimeout(resize,150) },{passive:true});

  resize();
  if(reduced){ draw(performance.now()) } else { start() }
})();
