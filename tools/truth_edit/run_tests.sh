#!/bin/sh
# truth_edit のテストを全部走らせる（ブラウザは使わない）。
#   sh tools/truth_edit/run_tests.sh
set -e
here=$(cd "$(dirname "$0")" && pwd)

echo "=== 1) 構文チェック（python / javascript） ==="
python3 -m py_compile "$here/truth_edit.py"
rm -rf "$here/__pycache__"
echo "py_compile: OK"

tmp_js=$(mktemp -t truth_edit_script_check).js
node -e '
const fs = require("fs"), path = require("path");
const html = fs.readFileSync(path.join(process.argv[1], "index.html"), "utf8");
const m = /<script>([\s\S]*?)<\/script>/.exec(html);
if (!m) { console.error("<script> がありません"); process.exit(1); }
fs.writeFileSync(process.argv[2], m[1], "utf8");
console.log("index.html の script: " + m[1].length + " 文字 -> " + process.argv[2]);
' "$here" "$tmp_js"
node --check "$tmp_js"
rm -f "$tmp_js"
echo "node --check: OK"

echo
echo "=== 2) parse / serialize の往復（純関数） ==="
node "$here/test_roundtrip.cjs"

echo
echo "=== 3) 画面側の操作（最小 DOM） ==="
node "$here/test_dom_smoke.cjs"

echo
echo "=== 4) サーバ（ETag / 409 / Host / bak / Range） ==="
python3 "$here/test_server.py"

echo
echo "=== すべて成功 ==="