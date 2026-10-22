import subprocess
import sys
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "check_public.py"


def repo(tmp_path, text):
    r = tmp_path / "r"
    r.mkdir()
    g = lambda *a: subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", *a], cwd=r, check=True,
                                  capture_output=True)
    g("init", "-q", "-b", "main")
    (r / "a.txt").write_text(text, encoding="utf-8")
    g("add", "a.txt")
    g("commit", "-q", "-m", "first")
    return r


def check(tmp_path, r):
    deny = tmp_path / "deny.txt"
    deny.write_text("# 注記\n北山精機\n", encoding="utf-8")
    return subprocess.run([sys.executable, str(SCRIPT), "--root", str(r)], capture_output=True, text=True,
                          env={"KIKITORI_DENYLIST": str(deny), "PATH": "/usr/bin:/bin"})


def test_check_public_finds_denied_word(tmp_path):
    r = check(tmp_path, repo(tmp_path, "北山精機さんの工場"))
    assert r.returncode == 1 and "北山精機" in r.stdout


def test_check_public_passes_clean_repo(tmp_path):
    r = check(tmp_path, repo(tmp_path, "こんにちは"))
    assert r.returncode == 0, r.stdout
