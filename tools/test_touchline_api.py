"""Offline client contract tests. No FM process or engine is started."""
import contextlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import unittest
from unittest.mock import patch

import touchline_api as api


class ClientTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.discovery = Path(self.tmp.name) / "api.json"
        self.requests = []
        self.mode = "ok"
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def reply(self, value, code=200):
                data = json.dumps(value, ensure_ascii=False).encode("utf-8")
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                if self.path == "/health":
                    owner.requests.append((self.path, self.headers.get("Authorization"), None))
                    return self.reply({"service": "touchline-editor", "ready": True,
                                       "protocol_version": 1, "pid": os.getpid()})
                owner.requests.append((self.path, self.headers.get("Authorization"), None))
                self.reply({"protocol_version": 1, "commands": [{"name": "search"}]})

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                owner.requests.append((self.path, self.headers.get("Authorization"), body))
                if self.headers.get("Authorization") != "Bearer test-secret":
                    return self.reply({"error": "unauthorized"}, 401)
                if owner.mode == "drop":
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.connection.close()
                    return
                if owner.mode == "http_error":
                    return self.reply({"error": "internal"}, 500)
                if owner.mode == "forbidden":
                    return self.reply({"error": "forbidden"}, 403)
                if owner.mode == "redirect":
                    self.send_response(302)
                    self.send_header("Location", "/should-not-be-followed")
                    self.end_headers()
                    return
                if self.path == "/v1/shutdown":
                    return self.reply({"ok": True, "shutdown": True})
                if owner.mode == "command_error":
                    return self.reply({"id": body["id"], "ok": False, "error": "stale preview"})
                if owner.mode == "wrong_id":
                    return self.reply({"id": body["id"] + 1, "ok": True, "data": {}})
                return self.reply({"id": body["id"], "ok": True, "data": body["args"]})

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.discovery.write_text(json.dumps({"base_url": f"http://127.0.0.1:{self.server.server_port}",
            "token": "test-secret", "pid": os.getpid(), "protocol_version": 1}), encoding="utf-8")
        self.client = api.Client(self.discovery, auto_start=False, timeout=2)

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.tmp.cleanup()

    def commands(self):
        return [r for r in self.requests if r[0] == "/v1/command"]

    def test_unicode_auth_and_stateful_ids(self):
        first = self.client.request("search", {"query": "김민수 · 레체"})
        second = self.client.request("status")
        self.assertEqual(first["data"]["query"], "김민수 · 레체")
        self.assertEqual([first["id"], second["id"]], [1, 2])
        self.assertTrue(all(r[1] is None for r in self.requests if r[0] == "/health"))
        self.assertTrue(all(r[1] == "Bearer test-secret" for r in self.commands()))

    def test_capabilities_requires_auth(self):
        self.assertEqual(self.client.capabilities()["commands"][0]["name"], "search")
        self.assertEqual(self.requests[-1][1], "Bearer test-secret")

    def test_apply_disconnect_is_uncertain_and_never_retried(self):
        self.mode = "drop"
        with self.assertRaisesRegex(api.UncertainOutcome, "history"):
            self.client.request("apply", {"token": "reviewed-token"})
        self.assertEqual(len(self.commands()), 1)

    def test_500_edit_is_uncertain_and_never_retried(self):
        self.mode = "http_error"
        with self.assertRaises(api.UncertainOutcome):
            self.client.request("batch_apply", {"token": "reviewed-token"})
        self.assertEqual(len(self.commands()), 1)

    def test_command_error_is_returned_without_retry(self):
        self.mode = "command_error"
        result = self.client.request("apply", {"token": "expired"})
        self.assertFalse(result["ok"])
        self.assertEqual(len(self.commands()), 1)

    def test_rejected_request_not_retried(self):
        self.mode = "forbidden"
        with self.assertRaisesRegex(api.HTTPFailure, "403"):
            self.client.request("status")
        self.assertEqual(len(self.commands()), 1)

    def test_redirect_not_followed(self):
        self.mode = "redirect"
        with self.assertRaises(api.HTTPFailure):
            self.client.request("apply", {"token": "test"})
        self.assertFalse(any(r[0] == "/should-not-be-followed" for r in self.requests))

    def test_wrong_response_id_is_uncertain(self):
        self.mode = "wrong_id"
        with self.assertRaises(api.UncertainOutcome):
            self.client.request("apply", {"token": "test"})
        self.assertEqual(len(self.commands()), 1)

    def test_non_loopback_discovery_never_receives_token(self):
        data = json.loads(self.discovery.read_text())
        data["base_url"] = "http://example.com:1234"
        self.discovery.write_text(json.dumps(data))
        with self.assertRaises(api.ClientError):
            self.client.request("status")
        self.assertEqual(self.requests, [])

    def test_busy_live_pid_never_starts_another_engine(self):
        client = api.Client(self.discovery, engine=__file__, auto_start=True)
        with patch.object(client, "_find_healthy", side_effect=api.ClientError("busy")), \
             patch.object(api.subprocess, "Popen") as launch:
            with self.assertRaisesRegex(api.ClientError, "No second engine"):
                client.request("status")
            launch.assert_not_called()

    def test_invalid_args_never_submitted(self):
        for args in ([], "text", 1):
            with self.assertRaises(api.ClientError):
                self.client.request("status", args)
        with self.assertRaises(ValueError):
            self.client.request("preview", {"value": float("nan")})
        self.assertEqual(self.requests, [])

    def test_cli_args_file_and_output_utf8(self):
        args = Path(self.tmp.name) / "args.json"
        output = Path(self.tmp.name) / "result.json"
        args.write_text('{"query":"레체"}', encoding="utf-8-sig")
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = api.main(["search", "--args-file", str(args), "--output", str(output),
                             "--discovery", str(self.discovery), "--no-start"])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["data"]["query"], "레체")
        self.assertNotIn("test-secret", stream.getvalue())

    def test_cli_command_failure_exit_one(self):
        self.mode = "command_error"
        with contextlib.redirect_stdout(io.StringIO()):
            code = api.main(["apply", "--discovery", str(self.discovery), "--no-start"])
        self.assertEqual(code, 1)

    def test_stop_is_explicit_and_does_not_submit_bridge_command(self):
        with patch.object(api.subprocess, "Popen") as launch:
            result = self.client.shutdown()
            launch.assert_not_called()
        self.assertTrue(result["shutdown"])
        self.assertEqual(self.commands(), [])


if __name__ == "__main__":
    unittest.main()
