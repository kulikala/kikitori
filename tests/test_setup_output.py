import json

import pytest

from kikitori import __version__
from kikitori.cli import emit, main as kikitori_main
from kikitori.pipeline import Segment
from kikitori.refine import catalog, evidence
from kikitori.refine.catalog import Hardware
from kikitori.refine.cli import main as refine_main
from kikitori.refine.setup import State, plan_setup, todo

GB = 2**30
HF = [evidence.QWEN, evidence.KANA]


def state(**kw):
    base = dict(os="Darwin", hardware=Hardware("apple", 64 * GB, "Apple Silicon"), refine=True, local=True,
                tty=True, free_ollama=500 * GB, free_hf=500 * GB)
    return State(**(base | kw))


def kinds(steps):
    return [(s.kind, s.arg) for s in todo(steps)]


def test_setup_fresh_mac_installs_starts_and_pulls():
    steps = plan_setup(state(hf_missing=HF))
    assert kinds(steps) == [("install", ""), ("start", ""), ("pull", "qwen3.8:27b-mlx"), ("pull", "tev1:0.8b-mxfp8"),
                            ("hf", evidence.QWEN), ("hf", evidence.KANA)]


def test_setup_second_run_has_nothing_to_do():
    s = state(api=True, version=(0, 40, 8), cli=True, models={"qwen3.8:27b-mlx": 1, "tev1:0.8b-mxfp8": 1})
    assert todo(plan_setup(s)) == []


def test_setup_existing_app_is_started_not_installed():
    assert kinds(plan_setup(state(app=True), no_hf=True))[:2] == [("start", ""), ("pull", "qwen3.8:27b-mlx")]


def test_setup_remote_never_installs_and_defaults_to_cpu_models():
    s = state(local=False, api=True, version=(0, 40, 8), hardware=Hardware("cpu", None, "不明"), hardware_given=False)
    steps = plan_setup(s, no_hf=True)
    assert kinds(steps) == [("pull", "tev1:0.8b-q8_0")]
    assert any("--hardware" in x.text for x in steps)


def test_setup_remote_not_answering_stops():
    assert plan_setup(state(local=False, api=False))[-1].kind == "stop"


def test_setup_needs_sudo_without_terminal_stops():
    s = state(os="Linux", hardware=Hardware("cpu", 16 * GB), tty=False, sudo_needed=True, sudo_ok=False)
    assert plan_setup(s)[-1].kind == "stop"
    s.sudo_ok = True
    assert ("install", "") in kinds(plan_setup(s))


def test_setup_missing_tools_stop_before_install():
    s = state(os="Linux", hardware=Hardware("cpu", 16 * GB), tools_missing=["zstd"])
    last = plan_setup(s)[-1]
    assert last.kind == "stop" and "zstd" in last.text


def test_setup_old_ollama_stops():
    last = plan_setup(state(api=True, version=(0, 39, 1)))[-1]
    assert last.kind == "stop" and "0.40.0" in last.text


def test_setup_low_disk_stops():
    last = plan_setup(state(api=True, version=(0, 40, 8), free_ollama=10 * GB))[-1]
    assert last.kind == "stop" and "空き" in last.text


def test_setup_without_refine_installs_nothing():
    steps = plan_setup(state(refine=False))
    assert todo(steps) == [] and "入れません" in steps[0].text


def test_setup_systemd_stopped_asks_user():
    s = state(os="Linux", hardware=Hardware("cpu", 16 * GB), cli=True, systemd=True)
    assert "systemctl" in plan_setup(s)[-1].text


SEGS = [Segment(0.0, 1.5, 0, "こんにちは"), Segment(61.25, 3725.5, 1, 'A<B & C --> D "引用" \\ 改 行')]


def out(capsys, fmt):
    emit(SEGS, fmt)
    return capsys.readouterr().out


def test_yaml_reads_back_as_json(capsys):
    yaml = pytest.importorskip("yaml")
    y = out(capsys, "yaml")
    assert "こんにちは" in y and "\\u2028" in y
    assert yaml.safe_load(y) == json.loads(out(capsys, "json"))


def test_vtt(capsys):
    v = out(capsys, "vtt")
    assert v.startswith("WEBVTT\n\n00:00:00.000 --> 00:00:01.500\n<v 話者1>こんにちは\n\n")
    assert "01:02:05.500" in v and "A&lt;B &amp; C --&gt; D" in v and "-->" not in v.split("\n")[-3]


def test_existing_formats_unchanged(capsys):
    assert out(capsys, "txt").startswith("[00:00.00] 話者1: こんにちは\n[01:01.25] 話者2: ")
    assert out(capsys, "srt").startswith("1\n00:00:00,000 --> 00:00:01,500\n話者1: こんにちは\n")


@pytest.mark.parametrize("main,name", [(kikitori_main, "kikitori"), (refine_main, "kikitori-refine")])
def test_version(main, name, capsys):
    with pytest.raises(SystemExit) as e:
        main(["--version"])
    assert e.value.code == 0 and capsys.readouterr().out.strip() == f"{name} {__version__}"


def test_refine_rejects_non_japanese(capsys):
    with pytest.raises(SystemExit) as e:
        refine_main(["--model", "ja-en", "--plan"])
    assert e.value.code == 2 and "日本語" in capsys.readouterr().err


def test_setup_continues_when_install_script_fails_after_installing(monkeypatch, tmp_path, capsys):
    # macOS の公式スクリプトは、Ollama.app を置いたあとの起動（open -a）で失敗を返すことがある
    from kikitori.refine import setup
    app = tmp_path / "Ollama.app"
    app.mkdir()
    monkeypatch.setattr(setup, "APP", app)
    monkeypatch.setattr(setup.subprocess, "run", lambda *a, **k: type("R", (), {"returncode": 1})())
    up = []
    monkeypatch.setattr(setup, "_start", lambda s: up.append(1) or True)
    monkeypatch.setattr(setup.ollama, "version", lambda: (0, 40, 8) if up else None)
    pulled = []
    monkeypatch.setattr(setup.ollama, "pull", lambda m, say: pulled.append(m))
    s = state(hardware=Hardware("apple", 24 * GB, "Apple Silicon"))
    assert setup.run_setup(s, plan_setup(s, no_hf=True), yes=True) == 0
    assert pulled == ["tev1:0.8b-mxfp8"]
    assert "Ollama は入りました" in capsys.readouterr().err


def test_setup_install_script_failure_without_ollama_fails(monkeypatch, tmp_path):
    from kikitori.refine import setup
    monkeypatch.setattr(setup, "APP", tmp_path / "missing.app")
    monkeypatch.setattr(setup.shutil, "which", lambda name: None)
    monkeypatch.setattr(setup.subprocess, "run", lambda *a, **k: type("R", (), {"returncode": 1})())
    s = state()
    assert setup.run_setup(s, plan_setup(s, no_hf=True), yes=True) == 1
