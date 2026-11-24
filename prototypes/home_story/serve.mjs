// ホームの「早見盤が主役のスクロール物語」案のひな形を配る。
// & "C:\Program Files\nodejs\node.exe" prototypes/home_story/serve.mjs → http://127.0.0.1:3080/
// 星のデータは本番と同じ src/api/home/static/ のものを読む（コピーを持たない）。
import http from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const FILES = {
  "/": [here("./index.html"), "text/html; charset=utf-8"],
  "/stars.json": [here("../../src/api/home/static/stars.json"), "application/json"],
  "/zodiac_lines.json": [here("../../src/api/home/static/zodiac_lines.json"), "application/json"],
};

http.createServer(async (req, res) => {
  const hit = FILES[new URL(req.url, "http://x").pathname];
  if (!hit) { res.writeHead(404); return res.end("not found"); }
  try {
    res.writeHead(200, { "content-type": hit[1], "cache-control": "no-store" });
    res.end(await readFile(hit[0]));
  } catch {
    res.writeHead(500); res.end();
  }
}).listen(3080, "127.0.0.1");
