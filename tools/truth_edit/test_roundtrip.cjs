#!/usr/bin/env node
/* index.html から <script> を取り出して、純関数（parse/serialize）を node で試す。
   使い方: node tools/truth_edit/test_roundtrip.cjs */
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const assert = require("assert");

const HERE = __dirname;
const INDEX = path.join(HERE, "index.html");
const SAMPLE = path.join(HERE, "sample", "sample.md");

function extractScript(html) {
  const m = /<script>([\s\S]*?)<\/script>/.exec(html);
  if (!m) throw new Error("<script> がありません");
  return m[1];
}

const html = fs.readFileSync(INDEX, "utf8");
const src = extractScript(html);
const tmp = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "truth_edit_js_")), "script.js");
fs.writeFileSync(tmp, src, "utf8");
console.log(`# index.html から script を取り出し: ${src.length} 文字 -> ${tmp}`);

const TE = require(tmp);
const sample = fs.readFileSync(SAMPLE, "utf8");

let pass = 0;
function ok(name, fn) {
  fn();
  pass += 1;
  console.log(`ok ${pass} - ${name}`);
}

ok("API が揃っている", () => {
  for (const k of ["parseSheet", "serializeSheet", "addBodyAfter", "deleteBody",
                   "setBodyText", "setStatus", "reorderBodyByTime", "formatStamp"]) {
    assert.strictEqual(typeof TE[k], "function", k + " が無い");
  }
});

ok("parse -> serialize が sample.md と 1 バイトも一致", () => {
  const doc = TE.parseSheet(sample);
  const out = TE.serializeSheet(doc);
  assert.ok(Buffer.from(out, "utf8").equals(Buffer.from(sample, "utf8")),
    "round-trip が一致しない");
  const b = TE.findBody(doc);
  assert.strictEqual(b.length, 4, "本文は 4 行のはず");
  assert.deepStrictEqual(b.map((x) => [x.stamp, x.text, x.qws.length]), [
    ["00:00", "こんにちはテストです", 1],
    ["00:04", "二行目の発言", 1],
    ["00:09", "三行目", 0],
    ["00:15", "四行目の長い発言がここに入ります", 1],
  ]);
  assert.strictEqual(TE.statusValue(doc), "未校正");
  const terms = doc.items.filter((i) => i.kind === "term");
  assert.strictEqual(terms.length, 1);
});

ok("front matter・見出し・埋め込みは 1 バイトも変わらない（行単位で一致）", () => {
  const doc = TE.parseSheet(sample);
  TE.setBodyText(doc, TE.findBody(doc)[1].id, "二行目の発言（修正）");
  const lines = TE.serializeSheet(doc).split("\n");
  const before = sample.split("\n");
  const diff = [];
  for (let i = 0; i < Math.max(lines.length, before.length); i++) {
    if (lines[i] !== before[i]) diff.push([i + 1, before[i], lines[i]]);
  }
  assert.strictEqual(diff.length, 1, "差分が 1 行だけのはず: " + JSON.stringify(diff));
  assert.ok(diff[0][2].includes("（修正）"));
});

ok("行を足す: 直後の行のあと（qw の後ろ）に入り、qw はなし", () => {
  const doc = TE.parseSheet(sample);
  const body = TE.findBody(doc);
  const it = TE.addBodyAfter(doc, body[1].id, 6);
  assert.strictEqual(it.stamp, "00:06", "時刻は再生位置から");
  assert.strictEqual(it.text, "");
  assert.strictEqual(it.qws.length, 0, "qw なし");
  const out = TE.serializeSheet(doc);
  const lines = out.split("\n");
  assert.strictEqual(lines[25], "- [00:06] ", "追加行は二行目の発言のあと（26 行目）");
  assert.strictEqual(lines[24], "  <!-- qw: 二行目の発言です。 -->", "元の行の qw が先に来る");
  assert.strictEqual(lines[26], "- [00:09] 三行目", "元 3 行目はそのまま");
  // 追加以外が変わっていないこと
  const a = sample.split("\n"), b = lines;
  assert.strictEqual(b.length, a.length + 1);
  assert.deepStrictEqual(b.slice(0, 25), a.slice(0, 25), "追加位置より前が同じ");
  assert.deepStrictEqual(b.slice(26), a.slice(25), "追加位置より後が同じ");
});

ok("足した行の時刻が乱れていても保存時に時刻順に並ぶ（qw は元の行に付いたまま）", () => {
  const doc = TE.parseSheet(sample);
  const body = TE.findBody(doc);
  const it = TE.addBodyAfter(doc, body[1].id, 65.4);
  assert.strictEqual(it.stamp, "01:05");
  const out = TE.serializeSheet(doc);
  const tail = out.slice(out.indexOf("## 本文")).split("\n");
  assert.deepStrictEqual(tail, [
    "## 本文",
    "",
    "- [00:00] こんにちはテストです",
    "  <!-- qw: こんにちは、テストです。 -->",
    "- [00:04] 二行目の発言",
    "  <!-- qw: 二行目の発言です。 -->",
    "- [00:09] 三行目",
    "- [00:15] 四行目の長い発言がここに入ります",
    "  <!-- qw: 四行目の長い発言が、ここに入ります。 -->",
    "- [01:05] ",
    "",
  ]);
  // 保存は出力だけ直す。doc.items は並べ替えない（フォーカスや IME を壊さないため）
  assert.strictEqual(TE.needsSort(doc.items), true, "doc.items は並べ替えない");
  assert.strictEqual(TE.serializeSheet(doc), out, "2 度目の保存も同じ内容");
});

ok("空の行だけ消せる（qw ごと消える）", () => {
  const doc = TE.parseSheet(sample);
  const body = TE.findBody(doc);
  assert.strictEqual(TE.deleteBody(doc, body[1].id), false, "空でない行は消えない");
  TE.setBodyText(doc, body[1].id, "");
  assert.strictEqual(TE.deleteBody(doc, body[1].id), true);
  const out = TE.serializeSheet(doc);
  assert.ok(!out.includes("二行目の発言"), "本文が消えている");
  assert.ok(!out.includes("qw: 二行目の発言です。"), "qw ごと消える");
  assert.ok(out.includes("qw: こんにちは、テストです。"), "他の qw は残る");
});

ok("時刻が崩れたら保存時に時刻順へ（qw は元の行に付いたまま）", () => {
  const doc = TE.parseSheet(sample);
  const b = TE.findBody(doc);
  TE.setBodyText(doc, b[0].id, "A-最初");
  TE.setBodyText(doc, b[2].id, "C-三番目");
  TE.setBodyText(doc, b[3].id, "D-四番目");
  b[0].stamp = "00:30";   // 並びをわざと崩す
  b[2].stamp = "00:05";
  b[3].stamp = "00:20";
  assert.strictEqual(TE.needsSort(doc.items), true);
  const out = TE.serializeSheet(doc);
  // 本文セクションの 7 行だけを取り出して比較する
  const cut = (s) => s.slice(s.indexOf("## 本文")).split("\n");
  assert.deepStrictEqual(cut(out), [
    "## 本文",
    "",
    "- [00:04] 二行目の発言",
    "  <!-- qw: 二行目の発言です。 -->",
    "- [00:05] C-三番目",
    "- [00:20] D-四番目",
    "  <!-- qw: 四行目の長い発言が、ここに入ります。 -->",
    "- [00:30] A-最初",
    "  <!-- qw: こんにちは、テストです。 -->",
    "",
  ]);
  // 保存は出力だけ直す。doc.items は並べ替えない（フォーカスや IME を壊さないため）
  assert.strictEqual(TE.needsSort(doc.items), true, "doc.items は並べ替えない");
  assert.strictEqual(TE.serializeSheet(doc), out, "2 度目の保存も同じ内容");
});

ok("既に時刻順なら並べ替えない（1 バイトも動かない）", () => {
  const doc = TE.parseSheet(sample);
  assert.strictEqual(TE.needsSort(doc.items), false);
  assert.ok(Buffer.from(TE.serializeSheet(doc), "utf8").equals(Buffer.from(sample, "utf8")));
});

ok("同じ時刻は行の順序を保つ（安定ソート）", () => {
  const doc = TE.parseSheet(sample);
  const b = TE.findBody(doc);
  TE.addBodyAfter(doc, b[0].id, 0);
  TE.setBodyText(doc, TE.findBody(doc)[1].id, "同時刻");
  const out = TE.serializeSheet(doc);
  const first = out.split("\n").slice(21, 26);
  assert.deepStrictEqual(first, [
    "- [00:00] こんにちはテストです",
    "  <!-- qw: こんにちは、テストです。 -->",
    "- [00:00] 同時刻",
    "- [00:04] 二行目の発言",
    "  <!-- qw: 二行目の発言です。 -->",
  ]);
});

ok("status を書き換える（インラインコメントは残る）", () => {
  const doc = TE.parseSheet(sample);
  TE.setStatus(doc, "校正済み");
  let out = TE.serializeSheet(doc);
  assert.ok(out.includes("status: 校正済み"));
  assert.ok(!out.includes("未校正"));
  assert.strictEqual(TE.statusValue(doc), "校正済み");
  assert.strictEqual(out.split("\n").length, sample.split("\n").length);
  TE.setStatus(doc, "未校正");
  assert.ok(Buffer.from(TE.serializeSheet(doc), "utf8").equals(Buffer.from(sample, "utf8")));

  const doc2 = TE.parseSheet(sample.replace("status: 未校正\n", "status: 未校正  #  transports\n"));
  TE.setStatus(doc2, "校正済み");
  assert.ok(TE.serializeSheet(doc2).includes("status: 校正済み  #  transports"));
});

ok("status 行が無くても追加できる", () => {
  const src = sample.replace("status: 未校正\n", "");
  const doc = TE.parseSheet(src);
  assert.strictEqual(TE.statusValue(doc), "");
  TE.setStatus(doc, "校正済み");
  const out = TE.serializeSheet(doc);
  assert.ok(out.includes("offset: 0:00 から 1 分\nstatus: 校正済み\n---"));
  assert.strictEqual(TE.statusValue(doc), "校正済み");
});

ok("固有名詞の追加・編集・削除は固有名詞の行だけを変える", () => {
  const doc = TE.parseSheet(sample);
  const terms = () => doc.items.filter((i) => i.kind === "term" && !i.deleted);
  assert.strictEqual(terms().length, 1);
  TE.setTermValue(doc, terms()[0].id, "kikitori");
  let out = TE.serializeSheet(doc);
  assert.ok(out.includes("- kikitori\n"), out);
  TE.addTerm(doc, "RIP");
  out = TE.serializeSheet(doc);
  assert.ok(out.includes("- kikitori\n- RIP\n"));
  assert.ok(out.indexOf("- RIP") < out.indexOf("## 本文"), "固有名詞セクションの中");
  TE.deleteTerm(doc, terms()[0].id);
  out = TE.serializeSheet(doc);
  assert.ok(!out.includes("kikitori"));
  assert.ok(out.includes("- RIP\n"));
});

ok("CRLF でも丸ごと往復する", () => {
  const crlf = sample.replace(/\n/g, "\r\n");
  const doc = TE.parseSheet(crlf);
  const out = TE.serializeSheet(doc);
  assert.ok(Buffer.from(out, "utf8").equals(Buffer.from(crlf, "utf8")));
  TE.setBodyText(doc, TE.findBody(doc)[0].id, "改");
  assert.ok(TE.serializeSheet(doc).includes("- [00:00] 改\r\n"));
});

ok("末尾に改行の無いシートも往復する", () => {
  const noNl = sample.replace(/\n$/, "");
  const doc = TE.parseSheet(noNl);
  assert.ok(Buffer.from(TE.serializeSheet(doc), "utf8").equals(Buffer.from(noNl, "utf8")));
});

ok("日本語・空白・記号を含む本文を往復する", () => {
  const weird = "---\nstatus: 未校正\n---\n\n## 固有名詞\n\n- 株式会社 テスト & Co.\n\n## 本文\n\n" +
    "- [00:00] 「はい」、そうです。〜 100% ？\n  <!-- qw: 「はい」そうです 100%？ -->\n" +
    "- [0:07] タブ\tと  山\n";
  const doc = TE.parseSheet(weird);
  assert.ok(Buffer.from(TE.serializeSheet(doc), "utf8").equals(Buffer.from(weird, "utf8")));
  assert.strictEqual(TE.findBody(doc)[1].text, "タブ\tと  山");
  assert.strictEqual(TE.findBody(doc)[1].stamp, "0:07");
  assert.strictEqual(TE.formatStamp(65.9, "m"), "01:05");
  assert.strictEqual(TE.formatStamp(3725, "m"), "1:02:05");
  assert.strictEqual(TE.parseStamp("01:05"), 65);
  assert.strictEqual(TE.parseStamp("1:02:05"), 3725);
});

ok("保存の順序（Enter → 次の行）の行移動 API", () => {
  const doc = TE.parseSheet(sample);
  const b = TE.findBody(doc);
  const next = TE.bodyNeighbour(doc, b[0].id, 1);
  assert.strictEqual(next.id, b[1].id);
  assert.strictEqual(TE.bodyNeighbour(doc, b[3].id, 1), null, "最後の行の先が無い");
  assert.strictEqual(TE.bodyNeighbour(doc, b[0].id, -1), null);
});


/* ---- 追加分（レビュー指摘 2 / 4 / 5 / 8） ---- */

ok("serialize は doc.items を並べ替えない（入力欄を壊さない）", () => {
  const doc = TE.parseSheet(sample);
  const before = doc.items.slice();
  const b = TE.findBody(doc);
  b[0].stamp = "00:50";
  const out = TE.serializeSheet(doc);
  assert.ok(out.indexOf("- [00:50] こんにちはテストです") > out.indexOf("- [00:04] 二行目の発言"));
  assert.strictEqual(doc.items.length, before.length);
  assert.strictEqual(doc.items[21], before[21], "同じオブジェクトのまま");
  assert.strictEqual(TE.findBody(doc)[0].stamp, "00:50", "並びは変わらない");
});

ok("末尾に改行の無いシート: 無変更でも往復する", () => {
  const noNl = sample.replace(/\n$/, "");
  const doc = TE.parseSheet(noNl);
  assert.ok(Buffer.from(TE.serializeSheet(doc), "utf8").equals(Buffer.from(noNl, "utf-8")));
});

ok("2: 末尾に改行の無いシートに足しても行がつながらない", () => {
  const noNl = sample.replace(/\n$/, "");
  const doc = TE.parseSheet(noNl);
  const last = TE.findBody(doc).pop();
  assert.strictEqual(last.stamp, "00:15", "最後の行は 00:15");
  TE.addBodyAfter(doc, last.id, 30);
  const out = TE.serializeSheet(doc);
  const lines = out.split("\n");
  const before = noNl.split("\n");
  assert.strictEqual(lines.length, before.length + 1, "行が 1 つ増えるだけ");
  assert.strictEqual(lines[lines.length - 1], "- [00:30] ", "新しい行は最後");
  assert.strictEqual(lines[lines.length - 2], "  <!-- qw: 四行目の長い発言が、ここに入ります。 -->");
  assert.deepStrictEqual(lines.slice(0, lines.length - 1), before, "元の行はそのまま 1 バイトも変えない");
  assert.ok(!out.endsWith("\n"), "末尾の改行は元のまま無し");
});

ok("2: 末尾に改行の無いシートを並べ替えても行がつながらない", () => {
  const noNl = sample.replace(/\n$/, "");
  const doc = TE.parseSheet(noNl);
  TE.findBody(doc)[0].stamp = "00:59";   // 先頭の行を末尾へ回す
  const out = TE.serializeSheet(doc);
  const lines = out.split("\n");
  assert.strictEqual(lines.length, noNl.split("\n").length, "行数は変わらない");
  assert.deepStrictEqual(lines.slice(21), [
    "- [00:04] 二行目の発言",
    "  <!-- qw: 二行目の発言です。 -->",
    "- [00:09] 三行目",
    "- [00:15] 四行目の長い発言がここに入ります",
    "  <!-- qw: 四行目の長い発言が、ここに入ります。 -->",
    "- [00:59] こんにちはテストです",
    "  <!-- qw: こんにちは、テストです。 -->",
  ]);
  assert.strictEqual(lines.slice(0, 21).join("\n"), noNl.split("\n").slice(0, 21).join("\n"));
  assert.ok(!out.endsWith("\n"), "末尾の改行は元のまま無し");
  assert.ok(!/-->- \[/.test(out), "qw 行と次の行がつながっていない");
});

ok("2: qw のある行の後ろに足したときは末尾の改行位置を譲る", () => {
  const noNl = sample.replace(/\n$/, "");
  const doc = TE.parseSheet(noNl);
  TE.addBodyAfter(doc, TE.findBody(doc)[3].id, 40);
  const out = TE.serializeSheet(doc);
  const lines = out.split("\n");
  assert.strictEqual(lines[lines.length - 1], "- [00:40] ", "新しい行が最後");
  assert.strictEqual(lines[lines.length - 2], "  <!-- qw: 四行目の長い発言が、ここに入ります。 -->");
  assert.ok(!out.endsWith("\n"));
});

ok("2: CRLF のシートでも足したり並べ替えたりしてもつながらない", () => {
  const crlf = sample.replace(/\n$/, "").replace(/\n/g, "\r\n");
  const doc = TE.parseSheet(crlf);
  TE.findBody(doc)[0].stamp = "00:59";
  TE.addBodyAfter(doc, TE.findBody(doc).pop().id, 44);
  const out = TE.serializeSheet(doc);
  assert.ok(!/(^|[^\r])\n/.test(out), "LF だけの改行が混ざらない");
  assert.ok(out.includes("- [00:44] "), "足した行がある");
  assert.ok(!out.endsWith("\r\n"), "末尾の改行は元のまま無し");
  const lines = out.split("\r\n");
  // 00:59 の行が最後になるので、追加した 00:44 の行はその前
  assert.strictEqual(lines[lines.length - 1], "  <!-- qw: こんにちは、テストです。 -->");
  assert.ok(lines.includes("- [00:44] "), "足した行がある（00:44）");
  assert.strictEqual(lines[lines.length - 2], "- [00:59] こんにちはテストです");
});

ok("4: 本文に改行が入らない（貼り付け・IME 対策）", () => {
  assert.strictEqual(TE.sanitizeBodyText("こんにちは\nテストです"), "こんにちは テストです");
  assert.strictEqual(TE.sanitizeBodyText("a\r\nb"), "a b");
  assert.strictEqual(TE.sanitizeBodyText("\n 両端 \n"), "両端");
  assert.strictEqual(TE.sanitizeBodyText("a\u2028b"), "a b");
  assert.strictEqual(TE.sanitizeBodyText("ふつうの文"), "ふつうの文");
  assert.strictEqual(TE.sanitizeBodyText("  二重 空白  "), "  二重 空白  ", "触らない入力はそのまま");
  const doc = TE.parseSheet(sample);
  const id = TE.findBody(doc)[0].id;
  TE.setBodyText(doc, id, " pasted\r\ntext\n");
  assert.strictEqual(TE.findBody(doc)[0].text, "pasted text");
  assert.ok(TE.serializeSheet(doc).includes("- [00:00] pasted text\n"));
});

ok("5: UTF-8 の BOM があれば読み書きで保つ", () => {
  const bomDoc = "\uFEFF" + sample;
  const doc = TE.parseSheet(bomDoc);
  assert.strictEqual(doc.bom, "\uFEFF");
  const out = TE.serializeSheet(doc);
  assert.strictEqual(out.charCodeAt(0), 0xFEFF);
  assert.ok(Buffer.from(out, "utf8").equals(Buffer.from(bomDoc, "utf8")), "往復で BOM が残る");
  TE.setBodyText(doc, TE.findBody(doc)[0].id, "直した");
  assert.strictEqual(TE.serializeSheet(doc).charCodeAt(0), 0xFEFF, "編集中も残る");
  const plain = TE.parseSheet(sample);
  assert.strictEqual(plain.bom, "");
  assert.strictEqual(TE.serializeSheet(plain).charCodeAt(0), 0x2d, "BOM 無しのシートに混ざらない");
  const doc2 = TE.parseSheet("\uFEFF" + sample.replace(/\n$/, ""));
  TE.addBodyAfter(doc2, TE.findBody(doc2).pop().id, 50);
  const out2 = TE.serializeSheet(doc2);
  assert.strictEqual(out2.charCodeAt(0), 0xFEFF);
  assert.ok(out2.endsWith("- [00:50] "), "BOM + 末尾改行無し + 足す");
});

fs.rmSync(path.dirname(tmp), { recursive: true, force: true });
console.log(`\n# ${pass} 件すべて成功`);