// フレームキル計算機 — 碁石盤(ウァン S3)。配置からスタック数を数え、計算用の疑似行に展開する。
//
// ルール(オーナー確認済み):
// - 盤は5x5。中央(RED_TILE_CELL)は赤マスで、碁石は置けない。線も中央で切れる。
// - 上下左右に隣の碁石がある碁石だけが起動する。起動していて、赤マスからマンハッタン距離2以内の
//   碁石が1回ずつダメージを出す。範囲外の碁石はダメージを出さないが、線の数には入る。
// - スタック数 = 横の線と縦の線のうち多い方の個数(2個以上の線だけ。自分も数える)。上限は素質の値。
// - 1スタックごとに倍率へ「1 + n×増加率」、術耐性から「n×貫通」(固定値)。

import { resolveTalentTableValue } from "./entry.js";
import { effectiveElite, effectiveModuleId } from "./stats.js";

export const STONE_BOARD_SIZE = 5;
export const RED_TILE_CELL = 12;
const STONE_RANGE = 2;

const cellX = (cell) => cell % STONE_BOARD_SIZE;
const cellY = (cell) => Math.floor(cell / STONE_BOARD_SIZE);

/** 赤マスからのマンハッタン距離。2以下なら碁石のダメージ範囲。 */
export function stoneRangeDistance(cell) {
  const center = Math.floor(STONE_BOARD_SIZE / 2);
  return Math.abs(cellX(cell) - center) + Math.abs(cellY(cell) - center);
}

export function isStoneInRange(cell) {
  return stoneRangeDistance(cell) <= STONE_RANGE;
}

/** 共有URL/localStorage由来の配列を「盤上の、赤マス以外の、重複しない整数」だけにする。 */
export function normalizeStones(stones) {
  if (!Array.isArray(stones)) return [];
  const valid = stones.filter((c) => Number.isInteger(c) && c >= 0 && c < STONE_BOARD_SIZE ** 2 && c !== RED_TILE_CELL);
  return [...new Set(valid)].sort((a, b) => a - b);
}

/** 行の育成設定で第二素質の値を引く。素質が未解放なら全部0(線のボーナスなし)。 */
export function resolveStoneBoardValues(op, entry, row) {
  const levels = {
    elite: effectiveElite(op, row),
    potential: row.potential ?? 5,
    moduleId: effectiveModuleId(op, row),
    moduleLevel: row.moduleLv,
  };
  const board = entry.stoneBoard;
  return {
    damagePerStack: resolveTalentTableValue(board.damagePerStack, levels),
    resPenPerStack: resolveTalentTableValue(board.resPenPerStack, levels),
    maxStacks: resolveTalentTableValue(board.maxStacks, levels),
  };
}

/**
 * 碁石ごとの状態を返す。`stacks`は起動していなければ0。
 * @returns {Array<{cell:number, active:boolean, fires:boolean, stacks:number}>}
 */
export function analyzeStoneBoard(stones, maxStacks) {
  const cells = new Set(normalizeStones(stones));
  const has = (x, y) =>
    x >= 0 && x < STONE_BOARD_SIZE && y >= 0 && y < STONE_BOARD_SIZE && cells.has(y * STONE_BOARD_SIZE + x);
  const lineLength = (x, y, dx, dy) => {
    let n = 1;
    for (let i = 1; has(x + dx * i, y + dy * i); i++) n++;
    for (let i = 1; has(x - dx * i, y - dy * i); i++) n++;
    return n >= 2 ? n : 0;
  };
  return [...cells].map((cell) => {
    const x = cellX(cell);
    const y = cellY(cell);
    const horizontal = lineLength(x, y, 1, 0);
    const vertical = lineLength(x, y, 0, 1);
    const active = horizontal + vertical > 0;
    return {
      cell,
      active,
      fires: active && isStoneInRange(cell),
      stacks: Math.min(Math.max(horizontal, vertical), maxStacks),
    };
  });
}

/**
 * 発動する碁石をスタック数ごとにまとめ、`computeRowDamage`に渡せる疑似行と敵にする。
 * 個数はHit数として渡す。発動する碁石が無ければHit数0の1件を返す(呼び出し側が空を扱わずに済む)。
 * @returns {Array<{partRow:object, partEnemy:object, stacks:number, count:number}>}
 */
export function expandStoneBoardParts(op, entry, row, enemy) {
  const { damagePerStack, resPenPerStack, maxStacks } = resolveStoneBoardValues(op, entry, row);
  const counts = new Map();
  for (const stone of analyzeStoneBoard(row.stones, maxStacks)) {
    if (stone.fires) counts.set(stone.stacks, (counts.get(stone.stacks) || 0) + 1);
  }
  if (counts.size === 0) return [{ partRow: { ...row, hits: 0 }, partEnemy: enemy, stacks: 0, count: 0 }];
  return [...counts.entries()]
    .sort(([a], [b]) => a - b)
    .map(([stacks, count]) => ({
      partRow: { ...row, multiplier: row.multiplier * (1 + stacks * damagePerStack), hits: count },
      partEnemy: { ...enemy, resFlat: (enemy.resFlat || 0) + stacks * resPenPerStack },
      stacks,
      count,
    }));
}
