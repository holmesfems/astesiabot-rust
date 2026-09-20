// Step 1 搬出: main.js が参照する文言オブジェクト T(strings) / P(phrases) / S(samples) の入れ物。
// main.js の import 時点では中身が空なので、ページ側の <script type="module"> が
// strings.{ja,en}.js / phrases.{ja,en}.js / samples.{ja,en}.js を import して
// installI18n() に渡し、boot() より前に埋める。
//
// オブジェクトを再代入せず Object.assign で埋めているのは、main.js 側が
// `import { T } from "./constants/i18n.js"` で束縛した参照をそのまま使い続けられるようにするため
// （再代入すると main.js 側の T はここでの再代入を追随できない = ライブバインディングの罠）。
export const T = {};
export const P = {};
export const S = {};

export function installI18n({ strings, phrases, samples }) {
  Object.assign(T, strings);
  Object.assign(P, phrases);
  Object.assign(S, samples);
}
