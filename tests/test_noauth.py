"""必要なモデルの配布元が、認証なしで取得できること（ネットワークが要るので -m network で実行）。"""
import os
import urllib.request

import pytest

from kikitori import models


@pytest.mark.network
@pytest.mark.parametrize("item", models.required("ja") + [models.ASR["ja-en"]], ids=lambda m: m[0])
def test_model_url_needs_no_token(item, monkeypatch):
    for k in ("HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"):
        monkeypatch.delenv(k, raising=False)
    req = urllib.request.Request(item[1], method="HEAD")
    with urllib.request.urlopen(req, timeout=30) as r:
        assert r.status == 200
    assert "huggingface.co" not in item[1]
