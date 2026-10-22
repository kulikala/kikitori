"""公開するものに、出してはいけない語（顧客名・取引先名・手元のパスの断片など）が無いかを調べる。1 件でもあれば 1 で終える。

禁止語の一覧はリポジトリの外に置く（一覧そのものが出してはいけない語のため）。
既定は ~/.config/kikitori/public-denylist.txt、環境変数 KIKITORI_DENYLIST で変えられる。1 行 1 語、# で始まる行は注記。

調べるもの:
- 公開する履歴（既定は main）の全コミットの、ファイルの中身・パス・コミットメッセージ・author と committer
- タグのメッセージ
- 作業ツリーのうち git が管理するファイル（git ls-files）
- --dist で渡したフォルダの wheel と sdist の中身
使い方: python scripts/check_public.py [--rev main] [--dist フォルダ]
"""
import argparse
import os
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, check=True).stdout.decode("utf-8", "replace")


def denylist():
    p = Path(os.environ.get("KIKITORI_DENYLIST") or Path.home() / ".config/kikitori/public-denylist.txt")
    if not p.exists():
        sys.exit(f"check_public: 禁止語の一覧がありません: {p}")
    words = [w.strip() for w in p.read_text(encoding="utf-8").splitlines() if w.strip() and not w.startswith("#")]
    if not words:
        sys.exit(f"check_public: 禁止語の一覧が空です: {p}")
    return words


def scan(where, text, words, hits):
    low = text.lower()
    for w in words:
        if w.lower() in low:
            hits.append(f"{where}: 「{w}」")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--rev", default="main")
    ap.add_argument("--dist")
    ap.add_argument("--root", help="調べるリポジトリ（既定はこのスクリプトのあるリポジトリ）")
    a = ap.parse_args(argv)
    global ROOT
    if a.root:
        ROOT = Path(a.root)
    words = denylist()
    hits = []
    for c in git("rev-list", a.rev).split():
        scan(f"{c[:8]} のメタデータ", git("log", "-1", "--format=%an %ae %cn %ce%n%B", c), words, hits)
        for path in git("ls-tree", "-r", "--name-only", c).splitlines():
            scan(f"{c[:8]} のパス", path, words, hits)
            scan(f"{c[:8]}:{path}", git("show", f"{c}:{path}"), words, hits)
    for t in git("tag", "-l").split():
        scan(f"タグ {t}", git("tag", "-l", "--format=%(contents)", t), words, hits)
    for path in git("ls-files").splitlines():
        f = ROOT / path
        scan(f"作業ツリー {path}", path, words, hits)
        if f.is_file():
            scan(f"作業ツリー {path}", f.read_bytes().decode("utf-8", "replace"), words, hits)
    if a.dist:
        for f in Path(a.dist).iterdir():
            if f.suffix == ".whl":
                with zipfile.ZipFile(f) as z:
                    for n in z.namelist():
                        scan(f"{f.name}:{n}", n + "\n" + z.read(n).decode("utf-8", "replace"), words, hits)
            elif f.name.endswith(".tar.gz"):
                with tarfile.open(f) as t:
                    for m in t.getmembers():
                        body = t.extractfile(m).read().decode("utf-8", "replace") if m.isfile() else ""
                        scan(f"{f.name}:{m.name}", m.name + "\n" + body, words, hits)
    hits = list(dict.fromkeys(hits))
    for h in hits[:50]:
        print(h)
    print(f"check_public: {len(hits)} 件" + ("（先頭 50 件）" if len(hits) > 50 else ""))
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
