import pytest

from kikitori import device


def test_cpu_request_prefers_ort_when_available(monkeypatch):
    monkeypatch.setattr(device, "ort_providers", lambda: ["CPUExecutionProvider"])
    assert device.candidates("cpu") == [("cpu", "ort", "cpu"), ("cpu", "sherpa", "cpu")]


def test_cpu_request_without_onnxruntime_uses_sherpa(monkeypatch):
    monkeypatch.setattr(device, "ort_providers", lambda: [])
    assert device.candidates("cpu") == [("cpu", "sherpa", "cpu")]


def test_explicit_gpu_falls_back_to_cpu_candidate(monkeypatch):
    monkeypatch.setattr(device, "ort_providers", lambda: ["CPUExecutionProvider"])
    assert device.candidates("cuda") == [("cuda", "ort", "cuda"), ("cpu", "ort", "cpu"), ("cpu", "sherpa", "cpu")]


def test_unavailable_provider_is_reported_not_raised():
    def make(backend, provider):
        raise RuntimeError("使えません")

    asr, dt, msg = device.try_candidate(make, "ort", "cuda", lambda a: None)
    assert asr is None and "使えません" in msg


def test_silent_fallback_warning_is_treated_as_failure():
    import os

    def make(backend, provider):
        os.write(2, b"Warning: provider not available, fallback to cpu\n")
        return object()

    asr, _, _ = device.try_candidate(make, "ort", "directml", lambda a: None)
    assert asr is None


def _choose_with(monkeypatch, gpu_time, cpu_time):
    import time as _t

    monkeypatch.setattr(device, "candidates", lambda r, m="ja", p="fp32": [("coreml", "ort", "coreml"), ("cpu", "ort", "cpu")])
    times = {"coreml": gpu_time, "cpu": cpu_time}

    class A:
        def __init__(self, k):
            self.k = k

    return device.choose(lambda b, p: A(p), lambda asr: _t.sleep(times[asr.k]), "auto")[0]


def test_choose_keeps_cpu_when_gpu_not_clearly_faster(monkeypatch):
    assert _choose_with(monkeypatch, 0.20, 0.19) == "cpu"


def test_choose_takes_gpu_when_clearly_faster(monkeypatch):
    assert _choose_with(monkeypatch, 0.05, 0.20) == "coreml"


def test_explicit_gpu_skips_timing(monkeypatch):
    monkeypatch.setattr(device, "candidates", lambda r, m="ja", p="fp32": [("cuda", "ort", "cuda"), ("cpu", "ort", "cpu")])

    class A:
        pass

    assert device.choose(lambda b, p: A(), lambda a: None, "cuda")[0] == "cuda"


@pytest.mark.parametrize("model,precision", [("ja-en", "fp32"), ("ja", "int8"), ("ja", "fp16")])
def test_unsupported_combinations_never_try_gpu(model, precision):
    assert device.candidates("auto", model, precision) == [("cpu", "sherpa", "cpu")]
    assert device.candidates("directml", model, precision) == [("cpu", "sherpa", "cpu")]


def test_choose_reports_real_reason_when_nothing_builds():
    def make(backend, provider):
        raise SystemExit("精度が対応していません")

    with pytest.raises(SystemExit) as e:
        device.choose(make, lambda a: None, "cpu")
    assert "精度が対応していません" in str(e.value)


def test_device_module_does_not_import_onnxruntime(monkeypatch):
    import sys

    monkeypatch.setitem(sys.modules, "onnxruntime", None)
    assert device.candidates("auto")[-1] == ("cpu", "sherpa", "cpu")
