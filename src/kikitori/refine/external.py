"""外部 LLM に A・B・K と用語集を渡して直させる。本文は PC の外へ出るので、利用者が指定したときだけ使う。
- bundle: 指示と問いを JSON で出す。呼び出し元の LLM（エージェント）が直し、答えを --apply で戻す。
- claude: claude CLI（Haiku）に渡す。
"""
import json
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

from . import prompts

CHUNK = 60  # 1 回に渡す区間の数
CLAUDE_MODEL = "claude-haiku-5-5"


def bundle(segs, A, B, K, glossary, chunk=CHUNK):
    return {
        "kind": "kikitori-refine/fixev",
        "how": ("system を指示、chunks の各 user を問いとして LLM に渡し、答え（「[番号] 本文」の行）を"
                "chunks の順につなげて kikitori-refine --apply にこのファイルと一緒に渡す。"),
        "system": prompts.system("fixev", glossary),
        "chunks": [{"ids": ids, "user": prompts.user("fixev", ids, A, B, K)} for ids in prompts.chunks(len(A), chunk)],
        "segments": [{"start": s.start, "end": s.end, "speaker": s.speaker, "text": a, "b": b, "k": k}
                     for s, a, b, k in zip(segs, A, B, K)],
    }


def apply(bundle_, reply):
    """答えの行を本文に戻す。答えが無い区間は A のまま。戻せなかった区間の番号も返す。"""
    got = prompts.parse(reply)
    segs = bundle_["segments"]
    miss = [i for i in range(len(segs)) if i not in got]
    return [got.get(i, s["text"]) for i, s in enumerate(segs)], miss


def claude(system, users, log=lambda m: None):
    def one(u):
        r = subprocess.run(["claude", "-p", "--model", CLAUDE_MODEL, "--system-prompt", system, "--tools", "",
                            "--no-session-persistence"], input=u, capture_output=True, text=True, timeout=600,
                           cwd=tempfile.gettempdir())  # 作業フォルダの指示ファイルを読ませない
        if r.returncode != 0:
            print(f"警告: claude CLI が失敗しました（{r.stderr.strip()[:200]}）", file=sys.stderr)
        return r.stdout
    log(f"claude CLI（{CLAUDE_MODEL}）に {len(users)} 回に分けて渡します")
    with ThreadPoolExecutor(4) as ex:
        return "\n".join(ex.map(one, users))


def dumps(b):
    return json.dumps(b, ensure_ascii=False, indent=1)
