#!/usr/bin/env node
/* index.html の画面側を、いちばん小さい DOM の入れ物で動かして主要な操作を確認する。
   ブラウザは使わない。使い方: node tools/truth_edit/test_dom_smoke.cjs */

"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const assert = require("assert");

/* ---------- 最小限の DOM ---------- */
class El {
  constructor(tag) {
    this.tagName = (tag || "div").toUpperCase();
    this.children = [];
    this.parent = null;
    this.dataset = {};
    this.style = {};
    this._text = "";
    this._class = "";
    this.listeners = {};
    this.value = "";
    this.selectionStart = 0;
    this.selectionEnd = 0;
    this.focused = false;
    this.disabled = false;
    this.checked = false;
    this.spellcheck = true;
    this.rows = 1;
    this.title = "";
    this.placeholder = "";
    this.scrollHeight = 20;
    this.played = 0;
    this.classList = {
      _el: this,
      add: (c) => { if (!this._has(c)) this._set(this._class + " " + c); },
      remove: (c) => this._set(this._class.split(/\s+/).filter((x) => x && x !== c).join(" ")),
      contains: (c) => this._has(c),
      toggle: (c, on) => {
        const want = on === undefined ? !this._has(c) : !!on;
        if (want) this.classList.add(c); else this.classList.remove(c);
        return want;
      },
    };
  }
  _has(c) { return this._class.split(/\s+/).includes(c); }
  _set(v) { this._class = String(v).trim(); }
  get className() { return this._class; }
  set className(v) { this._set(v); }
  get firstChild() { return this.children[0] || null; }
  get parentNode() { return this.parent || doc.body; }
  get lastChild() { return this.children[this.children.length - 1] || null; }
  get textContent() { return this._text + this.children.map((c) => c.textContent || "").join(""); }
  set textContent(v) { this._text = String(v); this.children = []; }
  appendChild(c) { if (c.parent) c.parent.removeChild(c); c.parent = this; this.children.push(c); return c; }
  removeChild(c) { const i = this.children.indexOf(c); if (i >= 0) this.children.splice(i, 1); c.parent = null; return c; }
  insertBefore(c, ref) {
    if (!ref) return this.appendChild(c);
    const i = this.children.indexOf(ref);
    if (i < 0) return this.appendChild(c);
    if (c.parent) c.parent.removeChild(c);
    c.parent = this;
    this.children.splice(i, 0, c);
    return c;
  }
  remove() { if (this.parent) this.parent.removeChild(this); }
  addEventListener(type, fn) { (this.listeners[type] = this.listeners[type] || []).push(fn); }
  removeEventListener() {}
  setAttribute(k, v) { this[k] = v; }
  getAttribute(k) { return this[k]; }
  removeAttribute() {}
  dispatch(type, ev) {
    (this.listeners[type] || []).forEach((fn) => fn(Object.assign({ type, target: this, preventDefault() { this.defaultPrevented = true; }, stopPropagation() {} }, ev)));
  }
  focus() { doc.activeElement = this; this.focused = true; this.dispatch("focus", {}); }
  blur() { if (doc.activeElement === this) doc.activeElement = doc.body; this.focused = false; }
  setSelectionRange(a, b) { this.selectionStart = a; this.selectionEnd = b; }
  scrollIntoView() {}
  getBoundingClientRect() { return { left: 0, top: 0, width: 100, height: 3 }; }
  querySelectorAll(sel) {
    if (sel === "[data-rate]") {
      if (!this._rateBtns) {
        this._rateBtns = ["1", "0.85", "0.7"].map((r) => {
          const b = new El("button");
          b.dataset.rate = r;
          b.classList.add("on");
          return b;
        });
      }
      return this._rateBtns;
    }
    return [];
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || new El("div"); }
  get offsetWidth() { return 1; }
}

class TextNode {
  constructor(t) { this._v = String(t); this.parent = null; }
  get textContent() { return this._v; }
  set textContent(v) { this._v = String(v); }
  get nodeValue() { return this._v; }
  set nodeValue(v) { this._v = String(v); }
}

class AudioEl extends El {
  constructor() {
    super("audio");
    this.readyState = 1;
    this.duration = 30;
    this._t = 0;
    this.paused = true;
    this.playbackRate = 1;
  }
  get currentTime() { return this._t; }
  // 本物のブラウザと同じく、currentTime を代入すると seeking が飛ぶ
  set currentTime(v) { this._t = v; this.dispatch("seeking", {}); }
  play() { this.paused = false; this.played++; return Promise.resolve(); }
  pause() { this.paused = true; }
  load() {}
  // 本物のブラウザと同じく、音声の終わりでは一時停止した状態になる
  end() { this.paused = true; this.dispatch("ended", {}); return this; }
}

const doc = {
  readyState: "complete",
  activeElement: null,
  body: new El("body"),
  documentElement: new El("html"),
  _byId: new Map(),
  getElementById(id) {
    if (id === "player") return (doc._byId.set("player", doc._byId.get("player") || new AudioEl()), doc._byId.get("player"));
    if (!doc._byId.has(id)) doc._byId.set(id, new El("div"));
    return doc._byId.get(id);
  },
  createElement(t) { return new El(t); },
  createTextNode(t) { return new TextNode(t); },
  querySelector(sel) { return sel === ".bar" ? barEl : doc.getElementById("bar"); },
  addEventListener(type, fn) { (doc._listeners[type] = doc._listeners[type] || []).push(fn); },
  _listeners: {},
  dispatch(type, ev) { (doc._listeners[type] || []).forEach((fn) => fn(ev)); },
};
const barEl = new El("div");
doc.activeElement = doc.body;
doc.body.setAttribute = () => {};
doc.documentElement.setAttribute = () => {};
doc.documentElement.removeAttribute = () => {};

const store = new Map();
global.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)),
};
global.window = { addEventListener() {} };
global.document = doc;

// 拟似サーバ: ETag / If-Match / 409 を本物と同じ仕様で再現する
const calls = [];
let sheetText = fs.readFileSync(path.join(__dirname, "sample", "sample.md"), "utf8");
let etagSeq = 1;
let serverEtag = '"' + (etagSeq++) + '"';
let serverHold = null;      // 保存を保留（編集中の編集をテストする）
let releaseHold = null;

function resp(status, body, extraHeaders) {
  const headers = extraHeaders || {};
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (n) => headers[n.toLowerCase()] || null },
    text: () => Promise.resolve(body),
  };
}
global.fetch = function (url, opts) {
  calls.push({ url, opts });
  if (!opts || opts.method !== "PUT") {
    return Promise.resolve(resp(200, sheetText, { etag: serverEtag }));
  }
  const sent = Object.assign({}, opts.headers || {});
  calls[calls.length - 1].headers = sent;
  const apply = () => {
    sheetText = opts.body;
    serverEtag = '"' + (etagSeq++) + '"';
    return Promise.resolve(resp(200, '{"ok":true,"bytes":' + opts.body.length + "}", { etag: serverEtag }));
  };
  if (serverHold) {
    // 保留中: If-Match を検証してから.Kind
    const hold = serverHold;
    serverHold = null;
    return hold.then(() => apply());
  }
  const want = sent["If-Match"];
  if (want && want !== "*" && want !== serverEtag) {
    return Promise.resolve(resp(409, '{"ok":false,"error":"他のプログラムが先に書き換えました。上書きしません。"}',
      { etag: serverEtag }));
  }
  return apply();
};

// requestAnimationFrame: 手動で進める
const frames = new Map();
let nextFrameId = 1;
global.requestAnimationFrame = function (cb) { const id = nextFrameId++; frames.set(id, cb); return id; };
global.cancelAnimationFrame = function (id) { frames.delete(id); };
function flushFrames(max) {
  let n = 0;
  while (frames.size && n < (max || 30)) {
    const first = frames.entries().next().value;
    frames.delete(first[0]);
    first[1](0);
    n += 1;
  }
  return n;
}

const src = /<script>([\s\S]*?)<\/script>/.exec(fs.readFileSync(path.join(__dirname, "index.html"), "utf8"))[1];
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "truth_edit_dom_"));
const tmpJs = path.join(tmpDir, "script.js");
fs.writeFileSync(tmpJs, src, "utf8");

/* ---------- キー入力の再現 ---------- */
function key(code, mods) {
  mods = mods || {};
  const ev = Object.assign({
    code, key: code,
    altKey: false, metaKey: false, ctrlKey: false, shiftKey: false,
    isComposing: false, repeat: false, keyCode: 0,
    preventDefault() { this.defaultPrevented = true; },
    stopPropagation() {},
  }, mods);
  doc.dispatch("keydown", ev);
  return ev;
}
const tick = () => new Promise((r) => setImmediate(() => setImmediate(r)));

let pass = 0;
async function ok(name, fn) {
  await fn();
  pass += 1;
  console.log(`ok ${pass} - ${name}`);
}

(async function main() {
  require(tmpJs);              // boot() が走る
  await tick();

  const rowsBox = doc.getElementById("rows");
  const player = doc.getElementById("player");
  const rowEls = () => rowsBox.children;

  await ok("シートを読み込んで本文 4 行が並ぶ", () => {
    assert.strictEqual(calls[0].url, "/sheet");
    assert.strictEqual(rowEls().length, 4, "4 行");
    assert.strictEqual(doc.getElementById("player").children.length, 0);
    assert.ok(rowEls()[0].className.includes("cur"), "1 行目が強調");
    assert.strictEqual(rowEls()[1].children[1].children[0].value, "二行目の発言");
  });

  await ok("qw が薄い色で表示され、本文の下にある", () => {
    const qw = rowEls()[0].children[1].children[1];
    assert.strictEqual(qw.textContent, "qwこんにちは、テストです。");
    assert.ok(qw.className.includes("qw"));
    const none = rowEls()[2].children[1].children[1];
    assert.ok(none.textContent.includes("qw なし"), "qw なし");
  });

  await ok("1 行目を選ぶとその区間（0.3 秒の余白付き）が再生される", () => {
    rowEls()[0].children[1].children[0].focus();   // Option+キーは本文の入力欄の中だけで効く
    player.currentTime = 0;
    player.played = 0;
    const ev = key("ArrowDown", { altKey: true });
    assert.ok(ev.defaultPrevented, "preventDefault される");
    assert.ok(rowEls()[1].className.includes("cur"), "2 行目が強調");
    assert.strictEqual(player.currentTime.toFixed(2), "3.70", "4 秒 - 0.3 秒");
    assert.strictEqual(player.paused, false);
  });

  await ok("区間の終わりに達したら先頭へ戻る（ループ）", () => {
    player.currentTime = 9.35;   // 次の行 9 秒 + 0.3 = 9.3
    player.dispatch("timeupdate", {});
    assert.ok(player.currentTime < 4, "2 行目の先頭 3.7 へ戻る: " + player.currentTime);
    assert.strictEqual(player.paused, false);
  });

  await ok("ループ OFF なら終わりで止まる", () => {
    const loop = doc.getElementById("loop");
    loop.checked = false;
    loop.dispatch("change", {});
    player.currentTime = 9.35;
    player.dispatch("timeupdate", {});
    assert.strictEqual(player.paused, true, "一時停止");
    loop.checked = true;
    loop.dispatch("change", {});
  });

  await ok("Esc で再生 / 一時停止", () => {
    player.paused = true;
    const a = key("Escape");
    assert.ok(a.defaultPrevented);
    assert.strictEqual(player.paused, false);
    key("Escape");
    assert.strictEqual(player.paused, true);
  });

  await ok("Option+Space で今の行を頭から", () => {
    player.currentTime = 5;
    key("Space", { altKey: true });
    assert.ok(Math.abs(player.currentTime - 3.7) < 0.01);
  });

  await ok("Option+左 で 2 秒戻る", () => {
    rowEls()[1].children[1].children[0].focus();
    player.currentTime = 10;
    key("ArrowLeft", { altKey: true });
    assert.strictEqual(player.currentTime, 8);
  });

  await ok("Option+S は 0.7 倍で 1 回だけ（ループしない）", () => {
    rowEls()[1].children[1].children[0].focus();
    player.paused = true;
    key("KeyS", { altKey: true });
    assert.strictEqual(player.playbackRate, 0.7);
    player.currentTime = 9.35;   // 今の行（00:04）の終わり = 9 秒 + 0.3
    player.dispatch("timeupdate", {});
    assert.strictEqual(player.paused, true, "1 回で止まる");
    assert.strictEqual(player.playbackRate, 1, "速さは戻る");
  });

  await ok("Option+/ でカーソル位置に 〔？〕", () => {
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    ta.value = "こんにちはテストです";
    ta.selectionStart = 8;
    ta.selectionEnd = 8;
    const ev = key("Slash", { altKey: true });
    assert.ok(ev.defaultPrevented);
    assert.strictEqual(ta.value, "こんにちはテスト〔？〕です");
    assert.strictEqual(ta.selectionStart, 11);
  });

  await ok("qw をクリックで同じ行の本文に写せる", () => {
    const row = rowEls()[1];
    const ta = row.children[1].children[0];
    ta.focus();
    ta.value = "二行目の";
    ta.selectionStart = ta.selectionEnd = 4;
    const qw = row.children[1].children[1];
    qw.dispatch("click", { target: qw });
    assert.strictEqual(ta.value, "二行目の二行目の発言です。");
    assert.strictEqual(ta.selectionStart, 13);
  });

  await ok("Enter で確定して次の行へ移り、次の行が再生される", async () => {
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    player.paused = true;
    const ev = key("Enter");
    assert.ok(ev.defaultPrevented);
    assert.ok(rowEls()[1].className.includes("cur"));
    assert.strictEqual(player.paused, false);
    assert.strictEqual(Math.round(player.currentTime), 4, "2 行目（00:04）の区間 - 0.3");
  });

  await ok("Shift+Enter で後ろに新しい行（時刻は再生位置・qw なし）", () => {
    const ta = rowEls()[1].children[1].children[0];
    ta.focus();
    player.currentTime = 12.7;
    key("Enter", { shiftKey: true });
    assert.strictEqual(rowEls().length, 5);
    const added = rowEls()[2];
    assert.strictEqual(added.children[1].children[0].value, "");
    assert.strictEqual(added.children[1].children[1].textContent, "（qw なし）");
    assert.ok(rowEls()[2].className.includes("cur"));
  });

  await ok("空の行は Option+Backspace で消える（埋めた行は消えない）", () => {
    const filled = rowEls()[3].children[1].children[0];
    filled.focus();
    filled.value = "埋めた行";
    key("Backspace", { altKey: true });
    assert.strictEqual(rowEls().length, 5, "埋めた行は消えない");
    const empty = rowEls()[2].children[1].children[0];
    empty.focus();
    empty.value = "";
    key("Backspace", { altKey: true });
    assert.strictEqual(rowEls().length, 4, "空の行は消える");
  });

  await ok("編集すると 2 秒後に自動保存、Enter のたびに保存", async () => {
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    ta.value = "直した本文";
    ta.dispatch("input", {});
    assert.strictEqual(doc.getElementById("save").textContent, "未保存");
    await new Promise((r) => setTimeout(r, 2300));
    const put = calls.filter((c) => c.opts && c.opts.method === "PUT").pop();
    assert.ok(put, "PUT している");
    assert.ok(put.opts.body.includes("- [00:00] 直した本文"), put.opts.body);
    assert.ok(doc.getElementById("save").textContent.startsWith("保存"), doc.getElementById("save").textContent);
  });

  await ok("本文以外は 1 バイトも変わらない", () => {
    const put = calls.filter((c) => c.opts && c.opts.method === "PUT").pop();
    const a = fs.readFileSync(path.join(__dirname, "sample", "sample.md"), "utf8").split("\n");
    const b = put.opts.body.split("\n");
    assert.strictEqual(b.length, a.length, "行数が同じ");
    const changed = [];
    for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) changed.push(i + 1);
    assert.deepStrictEqual(changed, [22, 24], "変えたのは本文の 2 行だけ（他は 1 バイトも変わらない）");
  });

  await ok("保存に失敗したら目立つ表示", async () => {
    const savedFetch = global.fetch;
    global.fetch = (u, o) => (o && o.method === "PUT")
      ? Promise.resolve({ ok: false, status: 500, text: () => Promise.resolve('{"ok":false,"error":"書き込み不可"}') })
      : savedFetch(u, o);
    const ta = rowEls()[3].children[1].children[0];
    ta.focus();
    ta.value = "失敗する行";
    ta.dispatch("input", {});
    await new Promise((r) => setTimeout(r, 2400));
    const bar = doc.getElementById("errbar");
    assert.strictEqual(doc.getElementById("save").textContent, "保存失敗");
    assert.strictEqual(bar.hidden, false);
    assert.ok(bar.textContent.includes("書き込み不可"), bar.textContent);
    assert.ok(doc.getElementById("save").className.includes("err"));
    global.fetch = savedFetch;
    bar.hidden = true;
  });

  await ok("Cmd+S / Ctrl+S で保存", async () => {
    const before = calls.length;
    const ev = key("KeyS", { metaKey: true });
    assert.ok(ev.defaultPrevented);
    await tick();
    assert.ok(calls.length > before, "PUT が飛ぶ");
    const ctrl = key("KeyS", { ctrlKey: true });
    assert.ok(ctrl.defaultPrevented);
  });

  await ok("「校正済み」チェックで front matter の status を書き換える", async () => {
    const done = doc.getElementById("done");
    assert.strictEqual(done.checked, false);
    done.checked = true;
    done.dispatch("change", {});
    assert.ok(doc.getElementById("errbar").hidden);
    await new Promise((r) => setTimeout(r, 2300));
    const put = calls.filter((c) => c.opts && c.opts.method === "PUT").pop();
    assert.ok(put.opts.body.includes("status: 校正済み"), put.opts.body.slice(0, 200));
    done.checked = false;
    done.dispatch("change", {});
    await new Promise((r) => setTimeout(r, 2300));
    assert.ok(calls.filter((c) => c.opts && c.opts.method === "PUT").pop().opts.body.includes("status: 未校正"));
  });

  await ok("足した行の時刻が乱れていても保存で並べ替えられ、表示もファイルに会う", async () => {
    // 00:09 の行に 00:12 の行を足す（前後の行の時刻より後）
    const ta = rowEls()[2].children[1].children[0];
    ta.focus();
    player.currentTime = 12.4;
    key("Enter", { shiftKey: true });
    assert.strictEqual(rowEls().length, 5);
    const fresh = rowEls()[3].children[1].children[0];
    fresh.value = "さっきの行";
    fresh.dispatch("input", {});
    await new Promise((r) => setTimeout(r, 2300));
    const put = calls.filter((c) => c.opts && c.opts.method === "PUT").pop();
    const stamps = put.opts.body.split("\n").filter((l) => /^- \[/.test(l)).map((l) => /^- \[(\d+:\d+)\]/.exec(l)[1]);
    assert.deepStrictEqual(stamps, ["00:00", "00:04", "00:09", "00:12", "00:15"], "ファイルは時刻順");
    // 表示も同じ順、各行の入力内容も持ち合っている
    const shown = rowEls().map((row) => [
      row.children[0].firstChild.nodeValue,
      row.children[1].children[0].value,
    ]);
    assert.deepStrictEqual(shown, [
      ["[00:00]", "直した本文"],
      ["[00:04]", "二行目の二行目の発言です。"],
      ["[00:09]", "埋めた行"],
      ["[00:12]", "さっきの行"],
      ["[00:15]", "失敗する行"],
    ]);
    // さっき足した空の行を消して元に戻す
    const emptyRow = rowEls()[3].children[1].children[0];
    emptyRow.focus();
    emptyRow.value = "";
    emptyRow.dispatch("input", {});
    key("Backspace", { altKey: true });
    assert.strictEqual(rowEls().length, 4, "空の行は消える");
    await new Promise((r) => setTimeout(r, 2300));
  });

  await ok("固有名詞を 1 行 1 語で編集・追加できる", () => {
    const terms = doc.getElementById("terms");
    assert.strictEqual(terms.children.length, 1);
    const input = terms.children[0].children[0];
    input.value = "kikitori";
    input.dispatch("input", {});
    doc.getElementById("addterm").dispatch("click", {});
    const inputs = terms.children.map((li) => li.children[0]);
    assert.strictEqual(inputs.length, 2);
    inputs[1].value = "truth_edit";
    inputs[1].dispatch("input", {});
    assert.ok(doc.getElementById("save").textContent === "未保存");
  });

  await ok("再生速度と余白が設定できる（0〜1 秒）", () => {
    const bar = doc.querySelector(".bar");
    const btns = bar.querySelectorAll("[data-rate]");
    btns[1].dispatch("click", {});
    assert.strictEqual(doc.getElementById("player").playbackRate, 0.85);
    assert.ok(btns[1].className.includes("on"));
    btns[0].dispatch("click", {});
    const pad = doc.getElementById("pad");
    pad.value = "1";
    pad.dispatch("input", {});
    assert.strictEqual(doc.getElementById("padval").textContent, "1.00s");
    pad.value = "0.3";
    pad.dispatch("input", {});
    assert.strictEqual(doc.getElementById("padval").textContent, "0.30s");
  });

  await ok("キーの一覧が常に欄にある（HTML と畳み込み）", () => {
    const html = fs.readFileSync(path.join(__dirname, "index.html"), "utf8");
    const box = /<aside class="legend"[^>]*>[\s\S]*?<\/aside>/.exec(html);
    assert.ok(box, "legend がある");
    for (const k of ["Enter", "Esc", "Option+Space", "Option+↓", "Option+↑", "Option+←",
                     "Option+S", "Option+/", "Shift+Enter", "Option+Backspace", "Cmd+S"]) {
      assert.ok(box[0].includes(k), k + " が載っている");
    }
    assert.ok(/position:\s*fixed/.test(html), "隅に固定");
    const legend = doc.getElementById("legend");
    assert.ok(!legend.className.includes("min"), "最初は開いている");
    doc.getElementById("legendbtn").dispatch("click", {});
    assert.ok(legend.className.includes("min"), "畳める");
  });


  /* ---- レビュー指摘の追加分 ---- */

  await ok("1: IME 変換中の Enter / Esc / Option+/ は無視する", () => {
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    const before = rowEls().indexOf(rowEls().find((r) => r.className.includes("cur")));
    const prev = player.currentTime;
    const playing = player.paused;

    key("Enter", { isComposing: true });
    assert.strictEqual(rowEls().indexOf(rowEls().find((r) => r.className.includes("cur"))), before,
      "変換中の Enter で進まない");
    const ev229 = key("Enter", { keyCode: 229 });   // Safari は keyCode 229 で IME 完了を送る
    assert.ok(!ev229.defaultPrevented, "keyCode 229 は何もしない");
    assert.strictEqual(rowEls().indexOf(rowEls().find((r) => r.className.includes("cur"))), before);
    key("Escape", { isComposing: true });
    assert.strictEqual(player.paused, playing, "変換中の Esc で一時停止しない");
    const textBefore = ta.value;
    key("Slash", { altKey: true, isComposing: true });
    assert.ok(!ta.value.includes("〔？〕"), "変換中は〔？〕を入れない");
    assert.strictEqual(ta.value, textBefore, "本文は変わらない");
  });

  await ok("1: Enter を長押ししても連続して進まない", () => {
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    key("Enter", { shiftKey: false, repeat: true });
    const curNow = () => rowEls().findIndex((r) => r.className.includes("cur"));
    assert.strictEqual(curNow(), 0, "repeat では動かない");
    key("Enter");
    assert.strictEqual(curNow(), 1, "1 回目だけ動く");
    key("Enter", { repeat: true });
    assert.strictEqual(curNow(), 1, "押し続けても進まない");
    assert.ok(key("Enter", { repeat: true }).defaultPrevented, "入力欄には改行を入れない");
    assert.strictEqual(rowEls()[1].children[1].children[0].value.includes("\n"), false);
  });

  await ok("6: 本文の入力欄の外では Option+キーに乗らない", async () => {
    doc.activeElement = doc.body;         // 入力欄にカーソルがない
    const before = rowEls().findIndex((r) => r.className.includes("cur"));
    const t0 = player.currentTime;
    for (const code of ["ArrowDown", "ArrowUp", "ArrowLeft", "KeyS", "Space", "Slash", "Backspace"]) {
      const ev = key(code, { altKey: true });
      assert.ok(!ev.defaultPrevented, code + " は preventDefault しない");
    }
    assert.strictEqual(rowEls().findIndex((r) => r.className.includes("cur")), before, "行が動かない");
    assert.strictEqual(player.currentTime, t0, "再生も動かない");
    // Cmd/Ctrl を併用したときは Option 扱いしない
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    const ev = key("ArrowDown", { altKey: true, metaKey: true });
    assert.ok(!ev.defaultPrevented);
    const ev2 = key("ArrowDown", { altKey: true, ctrlKey: true });
    assert.ok(!ev2.defaultPrevented);
    // Cmd+S はどこでも効く
    const ev3 = key("KeyS", { metaKey: true });
    assert.ok(ev3.defaultPrevented, "Cmd+S は入力欄の外でも効く");
    await tick();
  });

  await ok("4: 貼り付けた改行は空白になって 1 行に収まる", async () => {
    const ta = rowEls()[1].children[1].children[0];
    ta.focus();
    ta.value = "二行目の";
    ta.selectionStart = ta.selectionEnd = 4;
    let prevented = false;
    ta.dispatch("paste", {
      clipboardData: { getData: () => "最初\n次の行\r\n三行目" },
      preventDefault() { prevented = true; },
    });
    assert.ok(prevented, "既定の貼り付けは止める");
    assert.strictEqual(ta.value, "二行目の最初 次の行 三行目", "改行が空白に");
    assert.ok(!/[\r\n]/.test(ta.value), "本文に残らない");
    key("KeyS", { metaKey: true });
    await tick();
    const put = calls.filter((c) => c.opts && c.opts.method === "PUT").pop();
    assert.ok(put.opts.body.includes("- [00:04] 二行目の最初 次の行 三行目"), "ファイルにも 1 行で入る");
    const bodyLines = put.opts.body.split("\n").filter((l) => /^[ \t]*- \[/.test(l));
    assert.ok(bodyLines.every((l) => !/[\r\n]/.test(l)), "本文行は 1 行ずつ");
  });

  await ok("4: 入力欄に改行が入っても 1 行に収める", () => {
    const ta = rowEls()[2].children[1].children[0];
    ta.focus();
    ta.value = "三行目\n 余計な行";
    ta.selectionStart = 99;
    ta.selectionEnd = 99;
    ta.dispatch("input", {});
    assert.strictEqual(ta.value, "三行目  余計な行", "改行だけが空白に（既存の空白は触らない）");
    assert.ok(!/[\r\n]/.test(ta.value));
    ta.value = "";
    ta.dispatch("input", {});
  });

  await ok("8: Enter のたびに保存する", async () => {
    const before = calls.filter((c) => c.opts && c.opts.method === "PUT").length;
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    ta.value = "Enter で保存";
    ta.dispatch("input", {});
    key("Enter");
    await tick();
    const puts = calls.filter((c) => c.opts && c.opts.method === "PUT");
    assert.ok(puts.length > before, "Enter 直後に PUT する（2 秒待たない）");
    assert.ok(puts[puts.length - 1].opts.body.includes("- [00:00] Enter で保存"));
    assert.ok(!doc.getElementById("save").textContent.includes("未保存"), doc.getElementById("save").textContent);
  });

  await ok("8: 保存中に編集しても、その編集は失われず後で送られる", async () => {
    const isPut = (c) => c.opts && c.opts.method === "PUT";
    const putIndexAtStart = calls.filter(isPut).length;
    const ta = rowEls()[3].children[1].children[0];
    ta.focus();
    ta.value = "最初の編集";
    ta.dispatch("input", {});
    serverHold = new Promise((resolve) => { releaseHold = resolve; });
    key("KeyS", { metaKey: true });           // 保存を開始（サーバ側で保留させる）
    await tick();
    assert.strictEqual(doc.getElementById("save").textContent, "保存中…");
    // 保存の途中でさらに編集する
    ta.value = "保存中に直した";
    ta.dispatch("input", {});
    assert.strictEqual(doc.getElementById("save").textContent, "未保存", "編集中の状態が分かる");
    releaseHold();
    await tick();
    await tick();
    await new Promise((r) => setTimeout(r, 2300));
    await tick();
    const mine = calls.filter(isPut).slice(putIndexAtStart).map((c) => c.opts.body);
    assert.ok(mine.length >= 2, "続きの保存がある: " + mine.length);
    assert.ok(mine[0].includes("最初の編集"), "1 回目は最初の編集");
    assert.ok(mine[0].includes("保存中に直した") === false, "1 回目は保存前の内容");
    assert.ok(mine[mine.length - 1].includes("保存中に直した"),
      "保存中の編集も後で送られる: " + mine[mine.length - 1].slice(0, 120));
    assert.strictEqual(mine.length, 2, "続けて入った要求は 1 回にまとめられる");
    assert.ok(!doc.getElementById("save").textContent.includes("未保存"),
      doc.getElementById("save").textContent);
    serverHold = null;
  });

  await ok("3: 409（競合）なら上書きせず、目立つ警告と読み直しの案内", async () => {
    const lastIdx = rowEls().length - 1;
    const nRows = rowEls().length;
    const ta = rowEls()[lastIdx].children[1].children[0];
    ta.focus();
    ta.value = "衝突する編集";
    ta.dispatch("input", {});
    // 他のプログラムが先に書き換える
    serverEtag = '"external-' + (etagSeq++) + '"';
    const before = calls.length;
    key("KeyS", { metaKey: true });
    await tick();
    await tick();
    assert.ok(calls.length === before + 1, "PUT は 1 回だけ（，再说はしない）");
    const state = doc.getElementById("save");
    assert.strictEqual(state.textContent, "競合", state.textContent);
    const bar = doc.getElementById("errbar");
    assert.strictEqual(bar.hidden, false, "警告が出ている");
    assert.ok(bar.textContent.includes("衝突"), bar.textContent);
    assert.ok(bar.textContent.includes("読み直"), "読み直しの案内がある");
    assert.strictEqual(sheetText.includes("衝突する編集"), false, "ファイルには書かれていない");
    // さらに編集しても自動保存しない
    ta.value = "衝突後の追加";
    ta.dispatch("input", {});
    await new Promise((r) => setTimeout(r, 2300));
    assert.strictEqual(calls.length, before + 1, "409 の後は送らない");
    assert.strictEqual(sheetText.includes("衝突後の追加"), false);
    // 読み直しボタンで解消する
    const reloadBtn = bar.children.find((c) => c.tagName === "BUTTON");
    assert.ok(reloadBtn, "読み直すボタンがある");
    reloadBtn.dispatch("click", {});
    await tick();
    assert.ok(!doc.getElementById("errbar").hidden === false, "警告が消える");
    assert.strictEqual(sheetText.includes("衝突する編集"), false, "外部の書き換えが残る");
    assert.strictEqual(rowEls().length, nRows, "読み直し後は " + nRows + " 行");
  });

  await ok("7: 区間の終わりは requestAnimationFrame でも拾う", () => {
    rowEls()[0].children[1].children[0].focus();
    key("ArrowDown", { altKey: true });      // 2 行目（00:04）の区間 3.7〜9.3 を再生
    assert.strictEqual(player.paused, false);
    player.currentTime = 9.31;               // 9.3 の直後でまだ timeupdate は来ていない
    const n = flushFrames();
    assert.ok(n > 0, "フレーム、进んでいる");
    assert.ok(player.currentTime < 4, "先頭 3.7 へ戻された: " + player.currentTime);
    assert.strictEqual(player.paused, false);
  });

  await ok("7: 最後の行は音声が終わってもループする", () => {
    key("End", {});                          // 番兵（何も起きない）
    const last = rowEls().length - 1;
    while (rowEls().findIndex((r) => r.className.includes("cur")) !== last) {
      key("ArrowDown", { altKey: true });
    }
    const start = player.currentTime;
    assert.ok(Math.abs(start - (15 - 0.3)) < 0.01, "最後の行（00:15）から: " + start);
    player.end();
    assert.strictEqual(player.paused, false, "終わっても鳴り続ける");
    assert.ok(Math.abs(player.currentTime - (15 - 0.3)) < 0.01, "先頭へ戻る: " + player.currentTime);
    // ループ OFF なら止まる
    const loop = doc.getElementById("loop");
    loop.checked = false;
    loop.dispatch("change", {});
    player.end();
    assert.strictEqual(player.paused, true, "ループ OFF なら終わる");
    loop.checked = true;
    loop.dispatch("change", {});
    player.pause();
  });

  await ok("7: ループ中に区間外へシークしたら巻き戻さない", () => {
    rowEls()[0].children[1].children[0].focus();
    key("ArrowDown", { altKey: true });
    assert.strictEqual(player.paused, false);
    // ユーザーが 25 秒へシーク（区間 3.7〜9.3 の外）
    player.currentTime = 25;
    player.dispatch("seeking", {});
    assert.ok(player.currentTime >= 25, "巻き戻さない: " + player.currentTime);
    player.currentTime = 25.2;
    flushFrames();
    assert.ok(player.currentTime >= 25, "ループもしない: " + player.currentTime);
    assert.strictEqual(player.paused, false, "再生は続く");
    player.pause();
  });

  await ok("8: 入力中は並べ替えず、フォーカスが外れたタイミングで画面だけ揃える", async () => {
    const nRows = rowEls().length;
    // 一番最初の行に 00:40 の行を足す（並びが崩れる）
    const ta = rowEls()[0].children[1].children[0];
    ta.focus();
    player.currentTime = 40;
    key("Enter", { shiftKey: true });
    assert.strictEqual(rowEls().length, nRows + 1);
    const fresh = rowEls()[1];
    fresh.children[1].children[0].value = "後から足した行";
    fresh.children[1].children[0].dispatch("input", {});
    key("KeyS", { metaKey: true });
    await tick();
    // 入力中は並べ替えない（フォーカスが飛ばない）
    assert.strictEqual(rowEls()[1].children[1].children[0].value, "後から足した行",
      "入力中の行はそのまま");
    const putWhileTyping = calls.filter((c) => c.opts && c.opts.method === "PUT").pop();
    const stampsInFile = putWhileTyping.opts.body.split("\n")
      .filter((l) => /^- \[/.test(l)).map((l) => /^- \[(\d+:\d+)\]/.exec(l)[1]);
    assert.deepStrictEqual(stampsInFile, stampsInFile.slice().sort(),
      "ファイルの中だけは時刻順に並んでいる");
    // フォーカスが外れると画面も揃う
    doc.activeElement = doc.body;
    fresh.children[1].children[0].dispatch("focusout", {});
    await new Promise((r) => setTimeout(r, 10));
    const shown = rowEls().map((r) => r.children[0].firstChild.nodeValue);
    assert.deepStrictEqual(shown, shown.slice().sort(), "画面も時刻順: " + shown.join(" "));
    const moved = rowEls().find((r) => r.children[1].children[0].value === "後から足した行");
    assert.ok(moved, "行の入力内容はその行のまま");
    const idx = rowEls().indexOf(moved);
    assert.ok(idx === stampsInFile.indexOf("00:40"), "時刻の軽い行的位置に並んだ");
  });

  fs.rmSync(tmpDir, { recursive: true, force: true });
  console.log(`\n# ${pass} 件すべて成功`);
})().catch((err) => {
  console.error(err);
  process.exit(1);
});