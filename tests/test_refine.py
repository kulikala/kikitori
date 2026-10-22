import json

from kikitori.pipeline import Segment
from kikitori.refine import external, local, ollama, prompts
from kikitori.refine.cli import load_glossary, main
from kikitori.refine import catalog
from kikitori.refine.catalog import Hardware
from kikitori.refine.route import Facts, choose

GB = 2**30
BIG = {"qwen3.8:27b-mlx": 18 * GB, "tev1:0.8b-mxfp8": 1 * GB}
GGUF = {"qwen3.8:27b-q4_K_M": 18 * GB, "tev1:0.8b-q8_0": 1 * GB}
MAC128 = Hardware("apple", 128 * GB)


def test_route_without_evidence_is_plain_even_if_external():
    assert choose(Facts(evidence=False, external="bundle", ollama=BIG)).route == "plain"


def test_route_external_skips_local_models():
    p = choose(Facts(evidence=True, external="claude", ollama=BIG, systemone=True, glossary=True, hardware=MAC128))
    assert (p.route, p.model) == ("external", None)


def test_route_merge_when_big_model_fits():
    p = choose(Facts(evidence=True, ollama=BIG, hardware=Hardware("apple", 32 * GB)))
    assert (p.route, p.model) == ("merge", "qwen3.8:27b-mlx")


def test_route_merge_on_remote_ollama_uses_any_catalog_model():
    assert choose(Facts(evidence=True, ollama=GGUF, hardware=None)).model == "qwen3.8:27b-q4_K_M"


def test_route_small_mac_uses_tev1_glossary():
    p = choose(Facts(evidence=True, ollama=BIG, systemone=True, glossary=True, hardware=Hardware("apple", 16 * GB)))
    assert (p.route, p.model) == ("glossary", "tev1:0.8b-mxfp8")


def test_route_ignores_models_for_other_hardware():
    # Apple Silicon の PC に GGUF 版しか無ければ使わない（セットアップが選ぶものと同じ表を見る）
    p = choose(Facts(evidence=True, ollama=GGUF, systemone=True, glossary=True, hardware=MAC128))
    assert p.route == "rule"


def test_route_glossary_needs_glossary_systemone_and_tev1():
    small = {"tev1:0.8b-mxfp8": GB}
    f = dict(evidence=True, hardware=MAC128)
    assert choose(Facts(ollama=small, systemone=True, glossary=False, **f)).route == "rule"
    assert choose(Facts(ollama=small, systemone=False, glossary=True, **f)).route == "rule"
    assert choose(Facts(ollama={}, systemone=True, glossary=True, **f)).route == "rule"
    assert choose(Facts(ollama=small, systemone=True, glossary=True, **f)).route == "glossary"


def test_route_without_ollama_is_rule():
    assert choose(Facts(evidence=True, ollama=None)).route == "rule"
    p = choose(Facts(evidence=True, ollama=BIG, no_ollama=True))
    assert p.route == "rule" and not p.notes


def test_route_named_merge_model_missing_falls_back():
    p = choose(Facts(evidence=True, ollama=BIG, systemone=True, glossary=True, merge_model="nope:1b", hardware=MAC128))
    assert p.route == "glossary" and "nope:1b" in p.notes[0]


def test_pick_by_hardware():
    pick = lambda kind, gb: catalog.pick(Hardware(kind, gb * GB))[0]
    assert pick("apple", 16) == {"merge": None, "fix": "tev1:0.8b-mxfp8"}
    assert pick("apple", 32) == {"merge": "qwen3.8:27b-mlx", "fix": "tev1:0.8b-mxfp8"}
    assert pick("apple", 128)["merge"] == "qwen3.8:27b-mlx"
    assert pick("nvidia", 24) == {"merge": None, "fix": "tev1:0.8b-q8_0"}
    assert pick("nvidia", 32) == {"merge": "qwen3.8:27b-q4_K_M", "fix": "tev1:0.8b-q8_0"}
    assert pick("cpu", 16) == {"merge": None, "fix": "tev1:0.8b-q8_0"}
    assert pick("cpu", 256)["merge"] is None


def test_detect(monkeypatch):
    monkeypatch.delenv("KIKITORI_HARDWARE", raising=False)
    monkeypatch.setattr(catalog, "_ram", lambda: 64 * GB)
    # Rosetta 上の x86_64 の Python でも、hw.optional.arm64 で Apple Silicon と分かる
    monkeypatch.setattr(catalog.platform, "system", lambda: "Darwin")
    monkeypatch.setattr(catalog.platform, "machine", lambda: "x86_64")
    monkeypatch.setattr(catalog, "_run", lambda cmd: "1" if "hw.optional.arm64" in cmd else "")
    assert catalog.detect().kind == "apple"
    # NVIDIA は最大 1 枚の VRAM
    monkeypatch.setattr(catalog.platform, "system", lambda: "Linux")
    monkeypatch.setattr(catalog.shutil, "which", lambda x: "/usr/bin/nvidia-smi")
    monkeypatch.setattr(catalog, "_run", lambda cmd: "24576\n49152")
    hw = catalog.detect()
    assert (hw.kind, hw.memory) == ("nvidia", 48 * GB)
    monkeypatch.setattr(catalog.shutil, "which", lambda x: None)
    assert catalog.detect().kind == "cpu"
    monkeypatch.setenv("KIKITORI_HARDWARE", "apple")
    assert catalog.detect().kind == "apple"


def test_length_rule_takes_b_only_when_much_longer():
    A = ["はい", "今日は晴れです", ""]
    B = ["この商品は自社製品です", "今日は晴れです。明日も", "あの"]
    # 2 字 → 11 字は 2*1.6+3=6.2 を超える。7 字 → 10 字は超えない。空 → 2 字は 3 を超えない
    assert local.length_rule(A, B) == ["この商品は自社製品です", "今日は晴れです", ""]


def test_parse_reads_numbered_lines_only():
    got = prompts.parse("説明の行\n[0] こんにちは\n  [12]  さようなら \n[x] 無視")
    assert got == {0: "こんにちは", 12: "さようなら"}


def test_user_context_and_chunks():
    A, B, K = ["a0", "a1", "a2", "a3", "a4", "a5"], ["b"] * 6, ["k"] * 6
    u = prompts.user("fixev", [3], A, B, K)
    assert u.startswith("直前の発言（参考）: a0a1a2\n直後の発言（参考）: a4a5\n\n[3]\n本文: a3\nB: b\nK: k")
    assert prompts.chunks(5, 2) == [[0, 1], [2, 3], [4]]


def test_bundle_apply_round_trip_keeps_missing_as_a():
    segs = [Segment(0.0, 1.0, 0, "あ"), Segment(1.0, 2.0, 1, "い")]
    b = json.loads(external.dumps(external.bundle(segs, ["あ", "い"], ["ア", "イ"], ["ア", "イ"], [("北山精機", "")])))
    assert "用語集: 北山精機" in b["system"] and b["chunks"][0]["ids"] == [0, 1]
    texts, miss = external.apply(b, "[1] いい")
    assert texts == ["あ", "いい"] and miss == [0]


def test_apply_command(tmp_path, capsys):
    segs = [Segment(0.0, 1.0, 0, "あ")]
    p = tmp_path / "b.json"
    p.write_text(external.dumps(external.bundle(segs, ["あ"], ["ア"], ["ア"], [])))
    r = tmp_path / "r.txt"
    r.write_text("[0] ああ\n")
    main(["--apply", str(p), "--reply", str(r), "--format", "json"])
    assert json.loads(capsys.readouterr().out)[0]["text"] == "ああ"


def test_glossary_loader(tmp_path):
    p = tmp_path / "g.tsv"
    p.write_text("# 注記\n北山精機\t得意先\n白峰\n\n", encoding="utf-8")
    assert load_glossary(p) == [("北山精機", "得意先"), ("白峰", "")]


def test_glossary_candidates_follow_sound():
    gl = [("北山精機", "得意先")]
    assert local.candidates("北山正気さんの工場", "キタヤマセーキサンノコージョー", gl)[0][0] == "北山精機さんの工場"
    assert local.candidates("北山正気さんの工場", "アイウエオ", gl) == []


def test_ollama_base(monkeypatch):
    monkeypatch.delenv("KIKITORI_OLLAMA", raising=False)
    monkeypatch.delenv("OLLAMA_HOST", raising=False)
    assert ollama.base() == "http://localhost:11434" and ollama.is_local()
    monkeypatch.setenv("OLLAMA_HOST", "0.0.0.0")
    assert ollama.base() == "http://localhost:11434"
    monkeypatch.setenv("KIKITORI_OLLAMA", "https://llm.example")
    assert ollama.base() == "https://llm.example" and not ollama.is_local()
    assert ollama.base("box:9000") == "http://box:9000"
