// フレームキル計算機 — ATK・昇進・モジュール・スキル解放の判定と、敵の防御/術耐性の有効値。

/** row.moduleIdが指すモジュールのLv1〜3配列（見つからなければnull）。 */
export function findModule(op, moduleId) {
  if (!op || !moduleId) return null;
  return op.modules.find((m) => m.id === moduleId) ?? null;
}

/* ============================================================
   P6: 昇進(elite)/レベル(level)/信頼度(trust)を指定してベースATKを計算する。
   `op`(CatalogOperator/InspireSourceどちらも同じ形状)の`phases`
   (`[{maxLevel, atkMin, atkMax}, ...]`。インデックス0=E0)を線形補間して求める。
   詳細はオーナー確認済みの実測値で検証済み:
     - エーベンホルツ E2 Lv60 = 1134 + 59×(266/89) = 1310.34… → 四捨五入1310
     - シー E2 Lv71・信頼度100%・無凸モジュール・潜在+34 =
       (771→918の補間886.618→887) + 110(信頼度) + 34(潜在) = 1031
       (切り捨てなら1030になり実測と食い違うため、Math.round[四捨五入]が正しい)
   `op.phases`が無い(verify.mjs等が直接atkBaseを渡す簡易opオブジェクト)場合は
   `op.atkBase`をそのまま使う(elite/level/trustは無視。既存テストとの後方互換)。
   ============================================================ */

/** 昇進段階(0=E0/1=E1/2=E2)に対応する`op.phases`要素。範囲外/`phases`無しはnull。 */
export function phaseFor(op, elite) {
  const phases = (op && op.phases) || [];
  return phases[elite] ?? null;
}

/** そのopが到達できる最大昇進(0始まり)。`phases`が空なら0。 */
export function maxEliteFor(op) {
  const phases = (op && op.phases) || [];
  return Math.max(phases.length - 1, 0);
}

/** 指定昇進の最大レベル。`phases`に無ければ1(=クランプ計算の安全なフォールバック)。 */
export function maxLevelForElite(op, elite) {
  const phase = phaseFor(op, elite);
  return phase ? phase.maxLevel : 1;
}

/**
 * 昇進/レベル/信頼度からベースATKを計算する。Lv1〜Lv最大の間は線形補間し、
 * 四捨五入(`Math.round`)する。信頼度加算(`atkTrustMax × trust/100`)も同様に四捨五入し、
 * 補間後の値へ加算する(それぞれ丸めてから足す。詳細はファイル冒頭コメント参照)。
 * `level`は`phase.maxLevel`にクランプし、1未満にはしない。
 */
export function computeBaseAtk(op, elite, level, trust) {
  if (!op || !op.phases || !op.phases.length) return (op && op.atkBase) || 0;
  const phase = phaseFor(op, elite) ?? op.phases[op.phases.length - 1];
  const clampedLevel = Math.min(Math.max(level, 1), phase.maxLevel);
  const raw =
    phase.maxLevel <= 1
      ? phase.atkMax
      : phase.atkMin + ((clampedLevel - 1) / (phase.maxLevel - 1)) * (phase.atkMax - phase.atkMin);
  const trustAtk = Math.round((op.atkTrustMax || 0) * ((trust ?? 100) / 100));
  return Math.round(raw) + trustAtk;
}

/** row/cfgの`elite`(未設定なら`op`の最大昇進)。古い形(P1〜P5)のstate/共有URLとの
 * 互換のためのフォールバック(`dropStaleRows`が本来は補完するが、念のためここでも安全策)。 */
export function effectiveElite(op, rowOrCfg) {
  return rowOrCfg && rowOrCfg.elite != null ? rowOrCfg.elite : maxEliteFor(op);
}

/** row/cfgの`level`(未設定ならその昇進の最大レベル)。 */
function effectiveLevel(op, rowOrCfg, elite) {
  return rowOrCfg && rowOrCfg.level != null ? rowOrCfg.level : maxLevelForElite(op, elite);
}

/** モジュールがそのelite/levelで実際に装備可能か。`module.unlockPhase`/`unlockLevel`が
 * 無い(verify.mjs等の簡易モジュールオブジェクト)場合は常にtrue(既存テストとの後方互換)。 */
export function moduleUsable(module, elite, level) {
  if (!module) return false;
  if (elite < module.unlockPhase) return false;
  if (elite === module.unlockPhase && level < module.unlockLevel) return false;
  return true;
}

/**
 * row/cfgが選択したモジュールのうち、現在のelite/levelで実際に装備できているものだけを
 * 返す(装備不可なら`null`)。ATK計算・特殊強化のモジュール条件・鼓舞の自己%パーツの
 * モジュール条件は全てこれ経由でmoduleIdを参照する(選択はしていても未装備の間は
 * 効果に含めない。UIはヒントで理由を示す)。
 */
export function effectiveModuleId(op, rowOrCfg) {
  const module = findModule(op, rowOrCfg && rowOrCfg.moduleId);
  if (!module) return null;
  const elite = effectiveElite(op, rowOrCfg);
  const level = effectiveLevel(op, rowOrCfg, elite);
  return moduleUsable(module, elite, level) ? rowOrCfg.moduleId : null;
}

/** entryが要求する解放昇進(`op.skillUnlockPhase`にskill_numが載っていなければnull。
 * 素質行等スキルではないentryはそもそも載らない)。 */
export function requiredEliteForEntry(op, entry) {
  if (!op || !entry) return null;
  const found = (op.skillUnlockPhase || []).find(([num]) => num === entry.skillNum);
  return found ? found[1] : null;
}

/**
 * 現在の`row.elite`ではentryのスキルがまだ解放されていない場合の警告文(例:
 * "S3は昇進2で解放")。解放済み/判定できない(entryが無い・素質行等)場合はnull。
 * 警告が出ていても計算自体は続行する(オーナー方針。撃破可否の判定はしない)。
 */
export function skillUnlockWarning(op, entry, row) {
  const required = requiredEliteForEntry(op, entry);
  if (required == null) return null;
  const elite = effectiveElite(op, row);
  if (elite >= required) return null;
  const prefix = /^\d+$/.test(entry.skillNum) ? `S${entry.skillNum}` : entry.skillNum;
  return `${prefix}は昇進${required}で解放`;
}

/**
 * P7: 昇進(elite)ごとに実際に到達できるスキルLvの上限(ゲームの一般ルール。オペレーター/
 * スキルには依らない): E0はSLv4まで、E1はSLv7まで、特化1〜3(SLv8〜10)は昇進2が必要。
 */
export function maxSkillLevelForElite(elite) {
  if (elite >= 2) return 10;
  if (elite === 1) return 7;
  return 4;
}

/**
 * 現在の`elite`では`skillLevel`がまだ解放されていない場合の警告文(例:
 * "特化は昇進2で解放"/"SLv5以上は昇進1で解放")。解放済みならnull。
 * `skillUnlockWarning`(そのスキル自体の解放昇進)とは独立の判定で、両方出ることもある
 * (計算自体はどちらの警告が出ていても続行する。オーナー方針)。
 */
export function skillLevelWarning(elite, skillLevel) {
  if (skillLevel >= 8) return elite >= 2 ? null : "特化は昇進2で解放";
  if (skillLevel >= 5) return elite >= 1 ? null : "SLv5以上は昇進1で解放";
  return null;
}

/**
 * P8: 潜在ランク(0始まり。0=潜在1〜5=潜在6)からATK加算値を引く。範囲外はクランプする。
 * `op.atkPotentialByRank`(カタログ由来。長さ6の累積配列)があればそれをそのまま引く。
 * 無い(verify.mjs等の簡易opオブジェクト)場合は`op.atkPotential`(合計値)を
 * 「潜在0(潜在1)だけ+0、それ以外は全額」という単純な表として扱う後方互換フォールバック。
 */
export function resolveAtkPotential(op, potentialRank) {
  const rank = Math.min(Math.max(potentialRank ?? 5, 0), 5);
  if (!op) return 0;
  if (op.atkPotentialByRank) return op.atkPotentialByRank[rank] ?? 0;
  return rank > 0 ? op.atkPotential || 0 : 0;
}

/**
 * P8: 潜在ランク(`row.potential`/鼓舞ソースcfgの`potential`)の値を検証する。0〜5の数値なら
 * そのまま、そうでなければ(旧boolean形式・欠損値)既定値(5=潜在6)にリセットする
 * (`dropStaleRows`が使う。丁寧な移行はしない方針の詳細は同関数のコメント参照)。
 */
export function normalizePotentialRank(v) {
  return typeof v === "number" && v >= 0 && v <= 5 ? v : 5;
}

/** atk = ベースATK(昇進/レベル/信頼度) + (潜在) + (モジュール。装備可能な場合のみ)。 */
export function resolveAtk(op, row) {
  const elite = effectiveElite(op, row);
  const level = effectiveLevel(op, row, elite);
  const trust = row && row.trust != null ? row.trust : 100;
  const moduleId = effectiveModuleId(op, { moduleId: row.moduleId, elite, level });
  const module = findModule(op, moduleId);
  const moduleAtk = module ? module.atkByLevel[row.moduleLv - 1] ?? 0 : 0;
  return computeBaseAtk(op, elite, level, trust) + resolveAtkPotential(op, row.potential) + moduleAtk;
}

/**
 * 敵の防御有効値(defEff)。`ignoreDef`は行ごと(そのオペレーターの防御無視特性)なので
 * 引数で受け取る。0未満にはならない。
 */
export function resolveDefEff(enemy, ignoreDef) {
  return Math.max(enemy.def * (1 - enemy.defPct) - enemy.defFlat - ignoreDef, 0);
}

/** 敵の術耐性有効値(resEff)。0〜100にクランプする。 */
export function resolveResEff(enemy) {
  const raw = enemy.res - enemy.resFlat;
  return Math.min(Math.max(raw, 0), 100);
}
