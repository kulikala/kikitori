"""Ollama の向き先と問い合わせ。
向き先は KIKITORI_OLLAMA、無ければ Ollama 自身の OLLAMA_HOST、どちらも無ければ手元（localhost:11434）。
値は「http://host:port」「host:port」「host」「https://host」のどれでもよい。0.0.0.0（待ち受けの指定）は手元として扱う。
"""
import json
import os
import urllib.error
import urllib.request

DEFAULT = "http://localhost:11434"


def base(value=None):
    h = (value if value is not None else os.environ.get("KIKITORI_OLLAMA") or os.environ.get("OLLAMA_HOST") or DEFAULT).strip()
    if "://" not in h:
        h = "http://" + h
    scheme, rest = h.split("://", 1)
    rest = rest.rstrip("/")
    if scheme == "http" and ":" not in rest.rsplit("]", 1)[-1]:  # http でポートが無ければ Ollama の既定。https は 443 のまま
        rest += ":11434"
    if rest.startswith("0.0.0.0:"):
        rest = "localhost:" + rest.split(":", 1)[1]
    return f"{scheme}://{rest}"


def url(path):
    return base() + path


def is_local():
    host = base().split("://", 1)[1].rsplit(":", 1)[0]
    return host in ("localhost", "127.0.0.1", "[::1]")


def post(path, body, timeout):
    r = urllib.request.Request(url(path), data=json.dumps(body, ensure_ascii=False).encode(),
                               headers={"Content-Type": "application/json"})
    return json.loads(urllib.request.urlopen(r, timeout=timeout).read())


def models(timeout=3):
    """置いてあるモデルの {名前: バイト数}。つながらなければ None。"""
    try:
        d = json.loads(urllib.request.urlopen(url("/api/tags"), timeout=timeout).read())
    except (OSError, ValueError):
        return None
    return {m["name"]: m.get("size", 0) for m in d.get("models", [])}


def has_systemone(timeout=3):
    """tev1 の選択に使う /v1/systemone があるか。素の Ollama には無い。"""
    try:
        urllib.request.urlopen(urllib.request.Request(url("/v1/systemone"), data=b"{}",
                                                      headers={"Content-Type": "application/json"}), timeout=timeout)
    except urllib.error.HTTPError as e:
        return e.code not in (404, 405)
    except OSError:
        return False
    return True


def chat(model, system, user, num_ctx=32768, timeout=1200):
    body = {"model": model, "stream": False, "think": False, "options": {"temperature": 0, "num_ctx": num_ctx},
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]}
    return post("/api/chat", body, timeout)["message"]["content"]


def version(timeout=3):
    """Ollama の版を (0, 40, 8) の形で返す。答えなければ None。"""
    try:
        v = json.loads(urllib.request.urlopen(url("/api/version"), timeout=timeout).read())["version"]
    except (OSError, ValueError, KeyError):
        return None
    nums = []
    for p in v.split("-")[0].split(".")[:3]:
        nums.append(int(p) if p.isdigit() else 0)
    return tuple(nums + [0] * (3 - len(nums)))


def pull(model, progress=lambda m: None):
    """/api/pull で取得する。途中で切れても、打ち直せば続きから取得される。"""
    r = urllib.request.Request(url("/api/pull"), data=json.dumps({"model": model, "stream": True}).encode(),
                               headers={"Content-Type": "application/json"})
    last = None
    with urllib.request.urlopen(r, timeout=3600) as res:
        for line in res:
            d = json.loads(line)
            if "error" in d:
                raise RuntimeError(d["error"])
            msg = d.get("status", "")
            if d.get("total"):
                msg += f" {d.get('completed', 0) * 100 // d['total']}%"
            if msg != last:
                progress(msg)
                last = msg
