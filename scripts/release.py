"""リリースの候補を作る: 検査 → 版と CHANGELOG の書き換え → テスト → ビルド → 試し導入 → 公開物の検査 → コミットとタグ。
push はしない。最後に、公開に要るコマンドを出す。手順の説明は docs/RELEASING.md。
使い方: python scripts/release.py X.Y.Z
"""
import datetime
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INIT = ROOT / "src/kikitori/__init__.py"
CHANGELOG = ROOT / "CHANGELOG.md"
ROOT_MARK = "kikitori: public history root"


def git(*args, check=True):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=check).stdout.strip()


def fail(msg):
    sys.exit(f"release: {msg}")


def run(cmd, **kw):
    print("$", " ".join(map(str, cmd)), flush=True)
    return subprocess.run(cmd, cwd=ROOT, **kw).returncode == 0


def check_before(version):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        fail(f"版は X.Y.Z の形で指定してください: {version}")
    if git("status", "--porcelain"):
        fail("作業ツリーに未コミットの変更があります")
    if git("branch", "--show-current") != "main":
        fail("main で実行してください")
    roots = git("rev-list", "--max-parents=0", "main").split()
    if len(roots) != 1 or git("log", "-1", "--format=%s", roots[0]) != ROOT_MARK:
        fail(f"main の根のコミットが公開用の印（{ROOT_MARK}）ではありません。私的な履歴を公開しないよう止めます")
    if git("tag", "-l", f"v{version}"):
        fail(f"タグ v{version} は既にあります")
    m = re.search(r"^## \[Unreleased\]\n(.*?)(?=^## \[|\Z)", CHANGELOG.read_text(encoding="utf-8"), re.M | re.S)
    if not m or not m.group(1).strip():
        fail("CHANGELOG.md の [Unreleased] が空です。変更点を書いてから実行してください")


def rewrite(version):
    INIT.write_text(f'__version__ = "{version}"\n', encoding="utf-8")
    today = datetime.date.today().isoformat()
    s = CHANGELOG.read_text(encoding="utf-8")
    CHANGELOG.write_text(s.replace("## [Unreleased]\n", f"## [Unreleased]\n\n## [{version}] - {today}\n", 1), encoding="utf-8")


def verify(version):
    if not run([sys.executable, "-m", "pytest", "-q"]):
        return "テストが失敗しました"
    out = Path(tempfile.mkdtemp(prefix="kikitori-dist-"))
    if not run(["uv", "build", "--out-dir", str(out)]):
        return "ビルドが失敗しました"
    wheels = list(out.glob("*.whl"))
    if len(wheels) != 1:
        return f"wheel が 1 つではありません: {wheels}"
    tools = Path(tempfile.mkdtemp(prefix="kikitori-tool-"))
    env = os.environ | {"UV_TOOL_DIR": str(tools / "tools"), "UV_TOOL_BIN_DIR": str(tools / "bin")}
    if not run(["uv", "tool", "install", "--python", "3.12", str(wheels[0])], env=env):
        return "wheel から入れられませんでした"
    for name in ("kikitori", "kikitori-refine"):
        got = subprocess.run([str(tools / "bin" / name), "--version"], capture_output=True, text=True).stdout.strip()
        if got != f"{name} {version}":
            return f"{name} --version が {got!r} です（{version} のはず）"
    if not run([sys.executable, str(ROOT / "scripts/check_public.py"), "--dist", str(out)]):
        return "公開物の検査で禁止語が見つかりました"
    print(f"wheel と sdist: {out}")
    return None


def main():
    if len(sys.argv) != 2:
        fail("使い方: python scripts/release.py X.Y.Z")
    version = sys.argv[1]
    check_before(version)
    rewrite(version)
    err = verify(version)
    if err:
        git("checkout", "--", str(INIT.relative_to(ROOT)), str(CHANGELOG.relative_to(ROOT)))
        fail(f"{err}。版と CHANGELOG の書き換えを戻しました")
    git("add", str(INIT.relative_to(ROOT)), str(CHANGELOG.relative_to(ROOT)))
    git("commit", "-m", f"Release {version}")
    git("tag", "-a", f"v{version}", "-m", f"kikitori {version}")
    print(f"\nv{version} のコミットとタグを作りました。公開するには:\n  git push origin main v{version}")


if __name__ == "__main__":
    main()
