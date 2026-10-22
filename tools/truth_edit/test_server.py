#!/usr/bin/env python3
"""truth_edit.py のサーバ側のテスト（標準ライブラリのみ、ブラウザは使わない）。

    python3 tools/truth_edit/test_server.py
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import tempfile
import threading
import unittest
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = os.path.join(HERE, "sample", "sample.md")
SAMPLE_MP3 = os.path.join(HERE, "sample", "sample.mp3")

_spec = importlib.util.spec_from_file_location("truth_edit", os.path.join(HERE, "truth_edit.py"))
te = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(te)
te.QUIET = True  # アクセスログを出さない


class Result:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = headers
        self.body = body

    @property
    def text(self):
        return self.body.decode("utf-8")

    def json(self):
        return json.loads(self.text)

    def header(self, name):
        return self.headers.get(name)


def request(url, method="GET", body=None, headers=None):
    req = urllib.request.Request(url, data=body, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(req) as res:
            return Result(res.status, res.headers, res.read())
    except urllib.error.HTTPError as exc:
        return Result(exc.code, exc.headers, exc.read())


class ServerCase(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="truth_edit_test_")
        self.sheet = os.path.join(self.dir, "シート.md")
        with open(SAMPLE, "rb") as fh:
            self.original = fh.read()
        with open(self.sheet, "wb") as fh:
            fh.write(self.original)
        shutil.copyfile(SAMPLE_MP3, os.path.join(self.dir, "シート.mp3"))
        self.paths = te.Paths(self.sheet)
        self.httpd = te.TruthEditServer(("127.0.0.1", 0), te.Handler, self.paths)
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join(timeout=5)
        shutil.rmtree(self.dir, ignore_errors=True)

    def url(self, path):
        return "http://127.0.0.1:%d%s" % (self.port, path)

    def get_sheet(self, headers=None):
        return request(self.url("/sheet"), headers=headers)

    def put_sheet(self, body, headers=None):
        h = {"Content-Type": "text/plain; charset=utf-8"}
        h.update(headers or {})
        return request(self.url("/sheet"), "PUT", body, h)

    def on_disk(self):
        with open(self.sheet, "rb") as fh:
            return fh.read()


class TestSheetRoundtrip(ServerCase):
    def test_01_get_sheet_is_byte_identical(self):
        res = self.get_sheet()
        self.assertEqual(res.status, 200)
        self.assertEqual(res.body, self.original)
        self.assertEqual(res.header("Content-Type"), "text/plain; charset=utf-8")

    def test_01_put_unchanged_keeps_bytes(self):
        res = self.put_sheet(self.original)
        self.assertEqual(res.status, 200)
        self.assertTrue(res.json()["ok"])
        self.assertEqual(self.on_disk(), self.original, "PUT 後も 1 バイトも変わらない")

    def test_01_index_is_served(self):
        res = request(self.url("/"))
        self.assertEqual(res.status, 200)
        self.assertEqual(res.header("Content-Type"), "text/html; charset=utf-8")
        with open(os.path.join(HERE, "index.html"), "rb") as fh:
            self.assertEqual(res.body, fh.read())

    def test_02_range_request(self):
        res = request(self.url("/audio"), headers={"Range": "bytes=0-99"})
        self.assertEqual(res.status, 206)
        self.assertEqual(len(res.body), 100)
        self.assertEqual(res.header("Content-Range"), "bytes 0-99/%d" % os.path.getsize(SAMPLE_MP3))
        with open(SAMPLE_MP3, "rb") as fh:
            self.assertEqual(res.body, fh.read(100))

    def test_02_range_forms(self):
        size = os.path.getsize(SAMPLE_MP3)
        cases = [
            ("bytes=0-", 206, size),
            ("bytes=-50", 206, 50),
            ("bytes=%d-" % (size - 8), 206, 8),
            ("bytes=%d-" % size, 416, None),
            ("bytes=abc", 200, size),
        ]
        for spec, want_status, want_len in cases:
            with self.subTest(spec=spec):
                res = request(self.url("/audio"), headers={"Range": spec})
                self.assertEqual(res.status, want_status)
                if want_len is not None:
                    self.assertEqual(len(res.body), want_len)

    def test_11_audio_has_no_cache_control(self):
        res = request(self.url("/audio"), headers={"Range": "bytes=0-9"})
        self.assertIsNone(res.header("Cache-Control"), "音声に no-store を付けない")
        self.assertEqual(res.header("Accept-Ranges"), "bytes")
        # シートと HTML には付ける
        self.assertEqual(self.get_sheet().header("Cache-Control"), "no-store")
        self.assertEqual(request(self.url("/")).header("Cache-Control"), "no-store")

    def test_bom_is_kept_verbatim(self):
        body = "﻿" + self.original.decode("utf-8")
        raw = body.encode("utf-8")
        res = self.put_sheet(raw)
        self.assertEqual(res.status, 200)
        self.assertEqual(self.on_disk(), raw, "BOM ごとそのまま書く")


class TestConflict(ServerCase):
    def test_03_etag_present_and_stable(self):
        res = self.get_sheet()
        etag = res.header("ETag")
        self.assertTrue(etag, "ETag が付く")
        self.assertEqual(res.header("ETag"), self.get_sheet().header("ETag"))

    def test_03_stale_if_match_is_409_and_does_not_write(self):
        stale = self.get_sheet().header("ETag")
        # 別のプログラムが先に書き換える
        changed = self.original.replace("三行目".encode(), "三行目(外部)".encode())
        with open(self.sheet, "wb") as fh:
            fh.write(changed)
        current = self.get_sheet().header("ETag")
        self.assertNotEqual(stale, current)

        res = self.put_sheet("上書きするな\n".encode("utf-8"), headers={"If-Match": stale})
        self.assertEqual(res.status, 409)
        body = res.json()
        self.assertFalse(body["ok"])
        self.assertIn("先に書き換えました", body["error"])
        self.assertEqual(res.header("ETag"), current, "409 でも今の ETag を返す")
        self.assertIn("外部".encode(), self.on_disk(), "ファイルは書き換わらない")

    def test_03_fresh_if_match_succeeds_and_returns_new_etag(self):
        etag = self.get_sheet().header("ETag")
        res = self.put_sheet(self.original + "- [00:20] 追��\n".encode("utf-8"),
                             headers={"If-Match": etag})
        self.assertEqual(res.status, 200)
        new_etag = res.header("ETag")
        self.assertTrue(new_etag)
        self.assertNotEqual(new_etag, etag)
        self.assertEqual(self.get_sheet().header("ETag"), new_etag, "次の GET は新しい ETag")
        # 古い ETag はもう効かない
        self.assertEqual(
            self.put_sheet("x\n".encode("utf-8"), headers={"If-Match": etag}).status, 409
        )

    def test_03_if_match_star_and_missing_are_allowed(self):
        self.assertEqual(self.put_sheet(self.original, headers={"If-Match": "*"}).status, 200)
        self.assertEqual(self.put_sheet(self.original).status, 200)

    def test_03_weak_and_quoted_etags_compare_equal(self):
        etag = self.get_sheet().header("ETag")
        weak = "W/" + etag
        self.assertEqual(self.put_sheet(self.original, headers={"If-Match": weak}).status, 200)

    def test_03_error_closes_connection(self):
        etag = self.get_sheet().header("ETag")
        with open(self.sheet, "wb") as fh:
            fh.write("別\n".encode("utf-8"))
        res = self.put_sheet("x\n".encode("utf-8"), headers={"If-Match": etag})
        self.assertEqual(res.status, 409)
        self.assertEqual(res.header("Connection"), "close")
        res = request(self.url("/nothing"))
        self.assertEqual(res.status, 404)
        self.assertEqual(res.header("Connection"), "close")


class TestBackup(ServerCase):
    def test_09_bak_created_once(self):
        res = self.put_sheet(self.original)
        self.assertEqual(res.json()["backup"], "created")
        self.assertTrue(os.path.exists(self.sheet + ".bak"))
        with open(self.sheet + ".bak", "rb") as fh:
            self.assertEqual(fh.read(), self.original)

    def test_09_existing_bak_is_never_overwritten(self):
        keeper = "最初の原本\n".encode("utf-8")
        with open(self.sheet + ".bak", "wb") as fh:
            fh.write(keeper)
        for _ in range(3):
            res = self.put_sheet(self.original.replace("三行目".encode(), "直した".encode()))
            self.assertEqual(res.json()["backup"], "exists")
        with open(self.sheet + ".bak", "rb") as fh:
            self.assertEqual(fh.read(), keeper, ".bak は最初の原本のまま")

    def test_09_backup_failure_fails_the_save(self):
        original_copy2 = te.shutil.copy2

        def boom(src, dst):
            raise OSError("ディスクがいっぱい")

        te.shutil.copy2 = boom
        try:
            res = self.put_sheet("書けない\n".encode("utf-8"))
        finally:
            te.shutil.copy2 = original_copy2
        self.assertEqual(res.status, 500)
        self.assertFalse(res.json()["ok"])
        self.assertIn("保存できません", res.json()["error"])
        self.assertEqual(self.on_disk(), self.original, "ファイルは書き換わらない")
        self.assertFalse(os.path.exists(self.sheet + ".bak"))

    def test_09_survives_restart(self):
        self.put_sheet("1\n".encode("utf-8"))
        second = te.TruthEditServer(("127.0.0.1", 0), te.Handler, te.Paths(self.sheet))
        with open(self.sheet + ".bak", "rb") as fh:
            first_bak = fh.read()
        second.server_close()
        self.assertEqual(first_bak, self.original, "起動し直しても .bak を作り直さない")


class TestHostGuard(ServerCase):
    def test_10_bad_host_is_403(self):
        for host in ["evil.example.com", "attacker.com:80", "127.0.0.1.evil.com", "192.168.0.5"]:
            with self.subTest(host=host):
                res = request(self.url("/sheet"), headers={"Host": host})
                self.assertEqual(res.status, 403)
                self.assertEqual(res.header("Connection"), "close")
                self.assertFalse(res.json()["ok"])

    def test_10_bad_host_is_403_on_put_too(self):
        res = request(self.url("/sheet"), "PUT", "x\n".encode("utf-8"), {"Host": "evil.example.com"})
        self.assertEqual(res.status, 403)
        self.assertEqual(self.on_disk(), self.original)

    def test_10_good_hosts_are_allowed(self):
        for host in ["127.0.0.1:%d" % self.port, "localhost:%d" % self.port]:
            with self.subTest(host=host):
                self.assertEqual(request(self.url("/sheet"), headers={"Host": host}).status, 200)

    def test_10_wrong_port_is_403(self):
        res = request(self.url("/sheet"), headers={"Host": "127.0.0.1:1"})
        self.assertEqual(res.status, 403)

    def test_10_no_options_handler(self):
        res = request(self.url("/sheet"), "OPTIONS")
        self.assertEqual(res.status, 501, "do_OPTIONS は実装しない")


class TestPutGuards(ServerCase):
    def test_empty_body_is_rejected(self):
        self.assertEqual(self.put_sheet(b"").status, 400)

    def test_bad_utf8_is_rejected(self):
        res = self.put_sheet(b"\xff\xfe\x00")
        self.assertEqual(res.status, 400)
        self.assertEqual(self.on_disk(), self.original)

    def test_unknown_route(self):
        self.assertEqual(request(self.url("/nope")).status, 404)
        self.assertEqual(request(self.url("/nope"), "PUT", "x\n".encode("utf-8")).status, 404)

    def test_no_tmp_left_behind(self):
        self.put_sheet(self.original + "- [00:30] 追加\n".encode("utf-8"))
        leftovers = [n for n in os.listdir(self.dir) if ".tmp-" in n]
        self.assertEqual(leftovers, [])


if __name__ == "__main__":
    unittest.main(verbosity=2)