"""kikitori-refine --setup: 補正に使う環境（Ollama・Ollama のモデル・Hugging Face のモデル）を作る。

調べた事実（State）から、何をするかの並び（Step）を作る plan_setup は純粋な関数で、実行は run_setup が行う。
入れる前に、入れるものと容量を出して確認を求める。既にあるものは入れ直さない。
"""
import os
import platform
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

from . import catalog, evidence, ollama

GB = catalog.GB
OLLAMA_SIZE = 1 * GB  # Ollama 本体のおおよその大きさ
HF_SIZES = {evidence.QWEN: int(3.8 * GB), evidence.KANA: int(3.0 * GB)}
SPARE = 2 * GB  # 入れる量のほかに残しておく空き
TOOLS = {"Darwin": ["curl", "unzip"], "Linux": ["curl", "awk", "grep", "sed", "tee", "xargs", "zstd"], "Windows": []}
INSTALL = {"Darwin": ["sh", "-c", "curl -fsSL https://ollama.com/install.sh | sh"],
           "Linux": ["sh", "-c", "curl -fsSL https://ollama.com/install.sh | sh"],
           "Windows": ["powershell", "-NoProfile", "-Command", "irm https://ollama.com/install.ps1 | iex"]}


@dataclass
class State:
    os: str  # Darwin / Linux / Windows
    hardware: catalog.Hardware
    refine: bool  # Qwen3-ASR と kana-whisper を動かせる（kikitori[refine]）
    local: bool  # Ollama の向き先がこの PC
    api: bool = False  # Ollama の API が答える
    version: tuple | None = None
    cli: bool = False  # PATH に ollama がある
    app: bool = False  # macOS の /Applications/Ollama.app がある
    systemd: bool = False  # Linux で systemd の ollama サービスがある
    models: dict = field(default_factory=dict)  # Ollama にあるモデル
    tty: bool = False
    sudo_needed: bool = False
    sudo_ok: bool = False
    tools_missing: list = field(default_factory=list)
    free_ollama: int | None = None
    free_hf: int | None = None
    hf_missing: list = field(default_factory=list)
    hardware_given: bool = True  # 遠くの Ollama で --hardware を指定したか


@dataclass
class Step:
    kind: str  # note / stop / install / start / pull / hf
    text: str
    size: int = 0
    arg: str = ""


def _v(t):
    return ".".join(map(str, t))


def plan_setup(s: State, no_hf=False):
    steps = []
    if not s.refine:
        return [Step("note", "Qwen3-ASR と kana-whisper を動かせない（kikitori[refine] が無い）ので、補正に LLM を使いません。"
                             "Ollama とモデルは入れません")]
    hw = s.hardware
    steps.append(Step("note", f"機器: {hw.kind}（{hw.why}"
                              f"{'、' + str(round(hw.memory / GB)) + 'GB' if hw.memory else ''}）"))
    if not s.local and not s.hardware_given:
        steps.append(Step("note", "向き先の Ollama の機器が分からないので、GGUF の tev1 だけを入れます。"
                                  "Apple Silicon や NVIDIA なら --hardware apple / nvidia を付けてください"))
    will_install = False
    if s.local:
        if s.api or s.cli or s.app:
            steps.append(Step("note", "Ollama: 既にあります"))
        else:
            if s.tools_missing:
                cmd = ("brew install " if s.os == "Darwin" else "sudo apt-get install -y ") + " ".join(s.tools_missing)
                return steps + [Step("stop", f"Ollama を入れるのに要る道具がありません: {' '.join(s.tools_missing)}。"
                                             f"入れてから打ち直してください（例: {cmd}）")]
            if s.sudo_needed and not s.sudo_ok and not s.tty:
                return steps + [Step("stop", "Ollama の導入に管理者の権限（sudo）が要りますが、パスワードを聞ける端末がありません。"
                                             f"端末で次を打ってから、打ち直してください: {' '.join(INSTALL[s.os][2:])}")]
            steps.append(Step("install", "Ollama を公式の導入スクリプトで入れる", OLLAMA_SIZE))
            will_install = True
        if not s.api:
            if s.systemd and not will_install:
                return steps + [Step("stop", "Ollama のサービスが止まっています。sudo systemctl start ollama で起こしてから、打ち直してください")]
            steps.append(Step("start", "Ollama を起こす"))
    elif not s.api:
        return steps + [Step("stop", f"向き先の Ollama（{ollama.base()}）が答えません。KIKITORI_OLLAMA / OLLAMA_HOST を確かめてください")]
    if s.version and s.version < catalog.MIN_OLLAMA:
        how = "同じ導入スクリプト（curl -fsSL https://ollama.com/install.sh | sh）で更新できます" if s.local else "向き先の Ollama を更新してください"
        warn = "。macOS では、実行中の Ollama とモデルが止まります" if s.local and s.os == "Darwin" else ""
        return steps + [Step("stop", f"Ollama {_v(s.version)} は古く、{_v(catalog.MIN_OLLAMA)} 以上が要ります。{how}{warn}")]
    picks, why = catalog.pick(hw)
    steps += [Step("note", "モデルの選択: " + w) for w in why]
    pulls = [m for m in dict.fromkeys(picks.values()) if m]
    for m in pulls:
        if m in s.models:
            steps.append(Step("note", f"{m}: 既にあります"))
        else:
            steps.append(Step("pull", f"{m} を取得する", catalog.SIZES.get(m, 0), m))
    if no_hf:
        steps.append(Step("note", "Hugging Face のモデルは取得しません（--no-hf。初めて補正するときに取得されます）"))
    else:
        for repo in HF_SIZES:
            if repo in s.hf_missing:
                steps.append(Step("hf", f"{repo} を取得する", HF_SIZES[repo], repo))
            else:
                steps.append(Step("note", f"{repo}: 既にあります"))
    need_ollama = sum(x.size for x in steps if x.kind == "pull") if s.local else 0
    need_hf = sum(x.size for x in steps if x.kind == "hf")
    for need, free, where in ((need_ollama, s.free_ollama, "Ollama のモデルの置き場"), (need_hf, s.free_hf, "Hugging Face の置き場")):
        if need and free is not None and free < need + SPARE:
            return steps + [Step("stop", f"{where}の空きが {free / GB:.0f}GB で、{(need + SPARE) / GB:.0f}GB 要ります")]
    return steps


def todo(steps):
    return [x for x in steps if x.kind in ("install", "start", "pull", "hf")]


# --- 事実を調べる ---

def _tty():
    try:
        open("CON" if os.name == "nt" else "/dev/tty").close()
        return True
    except OSError:
        return False


def _free(path):
    p = Path(path)
    while not p.exists() and p != p.parent:
        p = p.parent
    try:
        return shutil.disk_usage(p).free
    except OSError:
        return None


def _hf_missing():
    from huggingface_hub import snapshot_download
    out = []
    for repo in HF_SIZES:
        try:
            snapshot_download(repo, local_files_only=True)
        except Exception:
            out.append(repo)
    return out


def gather(hardware_flag=None):
    osname = platform.system()
    local = ollama.is_local()
    hw = catalog.detect(hardware_flag) if (local or hardware_flag or os.environ.get("KIKITORI_HARDWARE")) \
        else catalog.Hardware("cpu", None, "向き先の機器が分からない")
    refine = evidence.available()
    v = ollama.version()
    s = State(os=osname, hardware=hw, refine=refine, local=local, api=v is not None, version=v,
              hardware_given=local or bool(hardware_flag or os.environ.get("KIKITORI_HARDWARE")))
    s.models = (ollama.models() or {}) if v else {}
    if local:
        s.cli = shutil.which("ollama") is not None
        s.app = osname == "Darwin" and Path("/Applications/Ollama.app").exists()
        s.systemd = osname == "Linux" and Path("/etc/systemd/system/ollama.service").exists()
        s.tools_missing = [t for t in TOOLS.get(osname, []) if not shutil.which(t)]
        s.tty = _tty()
        if osname == "Darwin":
            s.sudo_needed = not os.access("/usr/local/bin", os.W_OK)
        elif osname == "Linux":
            s.sudo_needed = os.geteuid() != 0
        if s.sudo_needed and shutil.which("sudo"):
            s.sudo_ok = subprocess.run(["sudo", "-n", "true"], capture_output=True).returncode == 0
        s.free_ollama = _free(os.environ.get("OLLAMA_MODELS") or Path.home() / ".ollama")
    if refine:
        s.hf_missing = _hf_missing()
        s.free_hf = _free(os.environ.get("HF_HOME") or Path.home() / ".cache" / "huggingface")
    return s


# --- 実行 ---

def _say(m):
    print(m, file=sys.stderr, flush=True)


def _confirm(total):
    try:
        with open("CON" if os.name == "nt" else "/dev/tty", "r+") as t:
            t.write(f"合わせて約 {total / GB:.1f}GB を入れます。進めますか [y/N]: ")
            t.flush()
            return t.readline().strip().lower() in ("y", "yes")
    except OSError:
        return False


def _start(s):
    if s.os == "Darwin" and Path("/Applications/Ollama.app").exists():
        subprocess.run(["open", "-a", "Ollama", "--args", "hidden"], check=False)
    else:
        exe = shutil.which("ollama") or "ollama"
        log = Path.home() / ".cache" / "kikitori" / "ollama.log"
        log.parent.mkdir(parents=True, exist_ok=True)
        kw = {"creationflags": 0x00000008 | 0x00000200} if os.name == "nt" else {"start_new_session": True}
        subprocess.Popen([exe, "serve"], stdout=open(log, "ab"), stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, **kw)
    for _ in range(30):
        if ollama.version():
            return True
        time.sleep(1)
    return False


def run_setup(s, steps, yes=False, dry_run=False):
    """0: 済んだ、1: 失敗、2: 止まった（要る物が足りない・確認が得られない）。"""
    for x in steps:
        if x.kind == "note":
            _say(f"- {x.text}")
        elif x.kind == "stop":
            _say(f"止まりました: {x.text}")
            return 2
        else:
            _say(f"+ {x.text}" + (f"（約 {x.size / GB:.1f}GB）" if x.size else ""))
    work = todo(steps)
    if not work:
        _say("全部あります。入れるものはありません")
        return 0
    if dry_run:
        _say("--dry-run のため、入れずに終えます")
        return 0
    total = sum(x.size for x in work)
    if not yes:
        if not s.tty:
            _say("端末が無いので確認できません。入れてよければ --yes を付けて打ち直してください")
            return 2
        if not _confirm(total):
            _say("入れずに終えます")
            return 2
    for x in work:
        _say(f"{x.text} ...")
        try:
            if x.kind == "install":
                if subprocess.run(INSTALL[s.os]).returncode != 0:
                    raise RuntimeError("公式の導入スクリプトが失敗しました")
            elif x.kind == "start":
                if not ollama.version() and not _start(s):
                    raise RuntimeError("30 秒待っても Ollama が答えません（~/.cache/kikitori/ollama.log を見てください）")
                v = ollama.version()
                if v and v < catalog.MIN_OLLAMA:
                    raise RuntimeError(f"Ollama {_v(v)} は古く、{_v(catalog.MIN_OLLAMA)} 以上が要ります")
            elif x.kind == "pull":
                if not ollama.version():
                    _start(s)
                ollama.pull(x.arg, lambda m: _say(f"  {m}"))
            elif x.kind == "hf":
                from huggingface_hub import snapshot_download
                snapshot_download(x.arg)
        except Exception as e:
            _say(f"失敗しました: {x.text}: {e}")
            _say("同じコマンドを打ち直すと、済んだところは飛ばし、取得は続きから進みます")
            return 1
    _say("済みました")
    return 0
