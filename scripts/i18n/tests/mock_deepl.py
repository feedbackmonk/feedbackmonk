"""A minimal in-process DeepL mock (http.server) for scripts/i18n/tests/*.

No real network, no real key, ever needed to exercise translate.py /
complete-plurals.py's HTTP paths. Handles exactly the two endpoints those
scripts call: GET /usage and POST /translate (form-encoded, DeepL's wire
format).
"""
from __future__ import annotations

import http.server
import json
import threading
import urllib.parse
from typing import Callable, List, Optional


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a) -> None:  # silence test output
        pass

    def _send_json(self, obj: dict) -> None:
        body = json.dumps(obj).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802 (stdlib method name)
        if self.path == "/usage":
            self._send_json(self.server.usage_response)  # type: ignore[attr-defined]
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/translate":
            self.send_response(404)
            self.end_headers()
            return
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length).decode("utf-8")
        params = urllib.parse.parse_qs(raw, keep_blank_values=True)
        texts: List[str] = params.get("text", [])
        target = params.get("target_lang", [""])[0]
        self.server.calls.append({  # type: ignore[attr-defined]
            "texts": list(texts),
            "target_lang": target,
            "tag_handling": params.get("tag_handling", [None])[0],
            "ignore_tags": params.get("ignore_tags", [None])[0],
            "formality": params.get("formality", [None])[0],
        })
        translate_fn = self.server.translate_fn  # type: ignore[attr-defined]
        translations = [{"text": translate_fn(t, target)} for t in texts]
        self._send_json({"translations": translations})


class MockDeepLServer:
    def __init__(self, usage_response: Optional[dict] = None,
                 translate_fn: Optional[Callable[[str, str], str]] = None):
        self.httpd = http.server.HTTPServer(("127.0.0.1", 0), _Handler)
        self.httpd.usage_response = usage_response or {"character_count": 0, "character_limit": 500_000}  # type: ignore[attr-defined]
        self.httpd.calls = []  # type: ignore[attr-defined]
        self.httpd.translate_fn = translate_fn or (lambda text, target: text)  # type: ignore[attr-defined]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def base_url(self) -> str:
        _, port = self.httpd.server_address
        return f"http://127.0.0.1:{port}"

    @property
    def calls(self) -> List[dict]:
        return self.httpd.calls  # type: ignore[attr-defined]

    def __enter__(self) -> "MockDeepLServer":
        self.thread.start()
        return self

    def __exit__(self, *exc_info) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()
