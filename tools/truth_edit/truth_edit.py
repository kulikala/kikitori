#!/usr/bin/env python3
"""文字起こし正解シートの聞き取り校正ツール。

    python3 tools/truth_edit/truth_edit.py <シート.md> [--port 8765]

- 127.0.0.1 だけで待ち受け、ブラウザは自動で開かない（URL を表示するだけ）。
- GET  /        -> index.html
    GET  /audio   -> シートと同じ名前の .mp3（Range リクエスト対応）
    GET  /sheet   -> シート本文 (UTF-8) + ETag
    PUT  /sheet   -> If-Match が古ければ 409。書けるときは一時ファイルへ書いてから rename
- シートの .bak は既にあれば作らない（最初の原本を守る）。コピーできなければ保存ごと失敗。
- パスにスペースや日本語が入っていても動く。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import threading
import urllib.parse
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_PUT_BYTES = 32 * 1024 * 1024
QUIET = False  # テスト中は True にしてアクセスログを黙らせる
CHUNK = 256 * 1024
RANGE_RE = re.compile(r"^bytes=(\d*)-(\d*)$")
ETAG_RE = re.compile(r'^(W/)?"?([^"]*)"?$')


# --------------------------------------------------------------------------- utils


def human_size(n: int) -> str:
    return "%.1f MB" % (n / 1024 / 1024) if n >= 1024 * 1024 else "%.1f KB" % (n / 1024)


def file_etag(data: bytes) -> str:
    """内容のハッシュ。mtime より確実（保存直後など取りこぼさない）。"""
    return '"' + hashlib.sha256(data).hexdigest()[:32] + '"'


def normalize_etag(value: str) -> str:
    """If-Match の値-mediatype 付きや W/ を落として比較用の文字列にする。"""
    if value is None:
        return ""
    m = ETAG_RE.match(value.strip())
    return m.group(2).strip() if m else value.strip()


class Paths:
    def __init__(self, sheet: str) -> None:
        self.sheet = sheet
        stem, _ = os.path.splitext(sheet)
        self.audio = stem + ".mp3"
        self.backup = sheet + ".bak"
        here = os.path.dirname(os.path.abspath(__file__))
        self.index = os.path.join(here, "index.html")


class TruthEditServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, addr, handler, paths: Paths) -> None:
        super().__init__(addr, handler)
        self.paths = paths
        self._backup_lock = threading.Lock()
        self._save_lock = threading.Lock()

    def ensure_backup(self) -> str:
        """<シート>.md.bak を 1 度だけ作る。既にあれば触らない。

        戻り値: created / exists
        例外: コピーに失敗したときは OSError（呼び出し側で保存ごと失敗させる）
        """
        with self._backup_lock:
            if os.path.exists(self.paths.backup):
                return "exists"
            shutil.copy2(self.paths.sheet, self.paths.backup)
            if not os.path.exists(self.paths.backup):
                raise OSError("バックアップを作れませんでした: " + self.paths.backup)
            return "created"


class Handler(BaseHTTPRequestHandler):
    server_version = "truth_edit/1.1"
    protocol_version = "HTTP/1.1"
    host_ok = True

    # ---- logging -----------------------------------------------------------
    def log_message(self, fmt, *args):  # noqa: A003 - stdlib hook
        if QUIET:
            return
        sys.stderr.write("  %s\n" % (fmt % args))
        sys.stderr.flush()

    # ---- Host --------------------------------------------------------------
    def _check_host(self) -> bool:
        """DNS rebinding 対策。127.0.0.1 / localhost だけ通す。"""
        host = self.headers.get("Host")
        if not host:
            return True  # HTTP/1.0（Host なし）は rebinding に使えない
        host = host.strip()
        if host.startswith("["):  # [::1]:8765
            closing = host.find("]")
            name, port = host[: closing + 1], host[closing + 1 :].lstrip(":")
        else:
            name, _, port = host.partition(":")
        if name not in ("127.0.0.1", "localhost", "[::1]"):
            return False
        if port:
            try:
                return int(port) == self.server.server_address[1]
            except ValueError:
                return False
        return True

    def parse_request(self):
        ok = super().parse_request()
        if ok:
            self.host_ok = self._check_host()
        return ok

    def guard(self) -> bool:
        if self.host_ok:
            return True
        self._send_json(
            HTTPStatus.FORBIDDEN,
            {"ok": False, "error": "このサーバーは 127.0.0.1 からしか使えません"},
            close=True,
        )
        return False

    # ---- small helpers -----------------------------------------------------
    def _route(self) -> str:
        path = urllib.parse.urlsplit(self.path).path
        try:
            path = urllib.parse.unquote(path)
        except Exception:
            pass
        if len(path) > 1 and path.endswith("/"):
            path = path[:-1]
        return path or "/"

    def _head(self, code: HTTPStatus, ctype: str, length: int, extra=None, no_store=True,
              close=False) -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(length))
        if no_store:
            self.send_header("Cache-Control", "no-store")
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        if close or int(code) >= 400:
            self.send_header("Connection", "close")
            self.close_connection = True
        self.end_headers()

    def _send_bytes(self, code: HTTPStatus, ctype: str, body: bytes, extra=None, no_store=True,
                    close=False) -> None:
        self._head(code, ctype, len(body), extra, no_store, close)
        if self.command != "HEAD" and body:
            self.wfile.write(body)

    def _send_json(self, code: HTTPStatus, obj, extra=None, close=False) -> None:
        body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self._send_bytes(code, "application/json; charset=utf-8", body, extra, True, close)

    def _not_found(self, what: str) -> None:
        self._send_json(HTTPStatus.NOT_FOUND, {"ok": False, "error": what + " が見つかりません"})

    # ---- dispatch ----------------------------------------------------------
    # do_OPTIONS は実装しない（未定義のメソッドは 501 のまま）
    def do_GET(self):  # noqa: N802
        if not self.guard():
            return
        self._dispatch_get()

    def do_HEAD(self):  # noqa: N802
        if not self.guard():
            return
        self._dispatch_get()

    def _dispatch_get(self):
        route = self._route()
        if route in ("/", "/index.html"):
            self._serve_static(self.server.paths.index, "text/html; charset=utf-8")
        elif route == "/sheet":
            self._serve_sheet()
        elif route == "/audio":
            self._serve_audio()
        elif route == "/favicon.ico":
            self.send_response(HTTPStatus.NO_CONTENT)
            self.send_header("Content-Length", "0")
            self.end_headers()
        else:
            self._not_found(route)

    def do_PUT(self):  # noqa: N802
        if not self.guard():
            return
        route = self._route()
        if route != "/sheet":
            self._not_found(route)
            return
        self._put_sheet()

    # ---- handlers ----------------------------------------------------------
    def _serve_static(self, path: str, ctype: str) -> None:
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError:
            self._send_json(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                {"ok": False, "error": "index.html が読めません: " + path},
            )
            return
        self._send_bytes(HTTPStatus.OK, ctype, data)

    def _serve_sheet(self) -> None:
        try:
            with open(self.server.paths.sheet, "rb") as fh:
                data = fh.read()
        except FileNotFoundError:
            self._not_found("シート")
            return
        except OSError as exc:
            self._send_json(HTTPStatus.INTERNAL_SERVER_ERROR, {"ok": False, "error": str(exc)})
            return
        self._send_bytes(
            HTTPStatus.OK, "text/plain; charset=utf-8", data, {"ETag": file_etag(data)}
        )

    def _serve_audio(self) -> None:
        path = self.server.paths.audio
        try:
            size = os.path.getsize(path)
        except OSError:
            self._not_found("音声 " + os.path.basename(path))
            return

        ctype = guess_audio_type(path)
        # 音声はキャッシュ閉じて剥がす（no-store を付けない）
        base = {"Accept-Ranges": "bytes"}
        store = False

        spec = self.headers.get("Range")
        if not spec:
            self._head(HTTPStatus.OK, ctype, size, base, store)
            if self.command == "HEAD":
                return
            self._stream_file(path, 0, size)
            return

        m = RANGE_RE.match(spec.strip())
        if not m:
            # 範囲形式が解釈できない場合は 200 で全量を返す（RFC 9110）。
            self._head(HTTPStatus.OK, ctype, size, base, store)
            if self.command == "HEAD":
                return
            self._stream_file(path, 0, size)
            return

        g1, g2 = m.group(1), m.group(2)
        if g1 == "" and g2 == "":
            self._range_not_satisfiable(size, base)
            return
        if g1 == "":  # bytes=-N （末尾から N バイト）
            length = int(g2)
            if length <= 0:
                self._range_not_satisfiable(size, base)
                return
            start = max(0, size - length)
            end = size - 1
        else:
            start = int(g1)
            end = int(g2) if g2 != "" else size - 1
        if size == 0 or start >= size or start > end:
            self._range_not_satisfiable(size, base)
            return
        end = min(end, size - 1)

        length = end - start + 1
        extra = dict(base)
        extra["Content-Range"] = "bytes %d-%d/%d" % (start, end, size)
        self._head(HTTPStatus.PARTIAL_CONTENT, ctype, length, extra, store)
        if self.command == "HEAD":
            return
        self._stream_file(path, start, length)

    def _range_not_satisfiable(self, size: int, base) -> None:
        extra = dict(base)
        extra["Content-Range"] = "bytes */%d" % size
        self._send_json(
            HTTPStatus.REQUESTED_RANGE_NOT_SATISFIABLE,
            {"ok": False, "error": "要求された範囲が収まりません"},
            extra,
            close=True,
        )

    def _stream_file(self, path: str, start: int, length: int) -> None:
        remaining = length
        try:
            with open(path, "rb") as fh:
                fh.seek(start)
                while remaining > 0:
                    chunk = fh.read(min(CHUNK, remaining))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    remaining -= len(chunk)
        except (BrokenPipeError, ConnectionResetError):
            pass
        except OSError as exc:
            self.log_message("audio read error: %s", exc)

    def _put_sheet(self) -> None:
        raw_len = self.headers.get("Content-Length")
        if raw_len is None:
            self._send_json(
                HTTPStatus.LENGTH_REQUIRED, {"ok": False, "error": "Content-Length がありません"}
            )
            return
        try:
            length = int(raw_len)
        except ValueError:
            self._send_json(
                HTTPStatus.BAD_REQUEST, {"ok": False, "error": "Content-Length が不正です"}
            )
            return
        if length <= 0:
            self._send_json(HTTPStatus.BAD_REQUEST, {"ok": False, "error": "本文が空です"})
            return
        if length > MAX_PUT_BYTES:
            self._send_json(
                HTTPStatus.REQUEST_ENTITY_TOO_LARGE,
                {"ok": False, "error": "大きすぎます（上限 %d バイト）" % MAX_PUT_BYTES},
            )
            return

        body = self.rfile.read(length)
        if len(body) != length:
            self._send_json(
                HTTPStatus.BAD_REQUEST, {"ok": False, "error": "本文が途中で切れました"}
            )
            return
        try:
            body.decode("utf-8")
        except UnicodeDecodeError as exc:
            self._send_json(
                HTTPStatus.BAD_REQUEST, {"ok": False, "error": "UTF-8 ではありません: %s" % exc}
            )
            return

        sheet = self.server.paths.sheet
        tmp = sheet + ".tmp-%d" % os.getpid()
        try:
            with self.server._save_lock:
                # If-Match: 読もうとした時と違う値なら誰かが先に書いた → 書かない
                if self.headers.get("If-Match"):
                    try:
                        with open(sheet, "rb") as fh:
                            current = file_etag(fh.read())
                    except OSError:
                        self._conflict(current="")
                        return
                    want = self.headers.get("If-Match").strip()
                    if want != "*" and normalize_etag(want) != normalize_etag(current):
                        self._conflict(current)
                        return

                # .bak は既にあれば触らない。作れなければ保存ごと失敗。
                backup = self.server.ensure_backup()

                with open(tmp, "wb") as fh:
                    fh.write(body)
                    fh.flush()
                    os.fsync(fh.fileno())
                os.replace(tmp, sheet)
        except OSError as exc:
            try:
                if os.path.exists(tmp):
                    os.unlink(tmp)
            except OSError:
                pass
            self._send_json(
                HTTPStatus.INTERNAL_SERVER_ERROR, {"ok": False, "error": "保存できません: %s" % exc}
            )
            return

        self._send_json(
            HTTPStatus.OK,
            {
                "ok": True,
                "bytes": length,
                "backup": backup,
                "backupPath": self.server.paths.backup,
            },
            {"ETag": file_etag(body)},
        )

    def _conflict(self, current: str) -> None:
        extra = {"ETag": current} if current else {}
        self._send_json(
            HTTPStatus.CONFLICT,
            {
                "ok": False,
                "error": "他のプログラムが先に書き換えました。上書きしません。"
                "読み直してから編集し直してください。",
                "etag": current,
            },
            extra,
        )


def guess_audio_type(path: str) -> str:
    ext = os.path.splitext(path)[1].lower()
    return {
        ".mp3": "audio/mpeg",
        ".m4a": "audio/mp4",
        ".mp4": "audio/mp4",
        ".wav": "audio/wav",
        ".aac": "audio/aac",
        ".ogg": "audio/ogg",
        ".opus": "audio/opus",
        ".flac": "audio/flac",
        ".webm": "audio/webm",
    }.get(ext, "application/octet-stream")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(
        prog="truth_edit.py",
        description="文字起こし正解シートを、音声を聞きながらキーボードだけで直す。",
    )
    ap.add_argument("sheet", help="正解シート（Markdown）のパス")
    ap.add_argument("--port", type=int, default=8765, help="待ち受けポート（既定 8765）")
    args = ap.parse_args(argv)

    sheet = os.path.abspath(os.path.expanduser(args.sheet))
    if not os.path.isfile(sheet):
        sys.stderr.write("エラー: シートが見つかりません: %s\n" % sheet)
        return 1

    paths = Paths(sheet)
    if not os.path.isfile(paths.index):
        sys.stderr.write("エラー: index.html が見つかりません: %s\n" % paths.index)
        return 1
    if not os.path.isfile(paths.audio):
        sys.stderr.write("警告: 音声が見つかりません（再生できません）: %s\n" % paths.audio)

    try:
        httpd = TruthEditServer(("127.0.0.1", args.port), Handler, paths)
    except OSError as exc:
        sys.stderr.write("エラー: ポート %d を使えません: %s\n" % (args.port, exc))
        return 1

    port = httpd.server_address[1]
    sys.stderr.write("truth_edit\n")
    sys.stderr.write("  シート: %s\n" % paths.sheet)
    if os.path.isfile(paths.audio):
        sys.stderr.write(
            "  音声:   %s (%s)\n" % (paths.audio, human_size(os.path.getsize(paths.audio)))
        )
    else:
        sys.stderr.write("  音声:   %s （見つかりません）\n" % paths.audio)
    sys.stderr.write("  開く:   http://127.0.0.1:%d/  （ブラウザは自動で開きません）\n" % port)
    sys.stderr.write("  終了:   Ctrl-C\n\n")
    sys.stderr.flush()

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        sys.stderr.write("\ntruth_edit: 終了します\n")
    finally:
        httpd.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())