"""Touchline's local semantic API client (Python standard library only).

This client never reads game memory, invents addresses, or retries a command.
Engine startup and health checks are separate from the requested operation.
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Any
import urllib.error
import urllib.parse
import urllib.request

PROTOCOL_VERSION = 1
MAX_RESPONSE_BYTES = 64 * 1024 * 1024
ROOT = Path(__file__).resolve().parents[1]


class ClientError(RuntimeError):
    pass


class UncertainOutcome(ClientError):
    """A submitted command may have executed. It must not be replayed blindly."""


class HTTPFailure(ClientError):
    def __init__(self, code: int):
        self.code = code
        super().__init__(f"Local API returned HTTP {code}; the command was not retried.")


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def default_discovery() -> Path:
    local = os.environ.get("LOCALAPPDATA")
    if not local:
        raise ClientError("LOCALAPPDATA is not set; pass --discovery explicitly.")
    return Path(local) / "TouchlineEditor" / "api.json"


def default_engine() -> Path:
    packaged = ROOT / "engine" / "Touchline Engine.exe"
    if packaged.is_file():
        return packaged
    # Deliberately do not fall back to an older GUI release without API support.
    return ROOT / "dist" / "api" / "Touchline Engine.exe"


def _process_exists(pid: int) -> bool:
    if os.name == "nt":
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
        kernel.GetExitCodeProcess.restype = wintypes.BOOL
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = kernel.OpenProcess(0x1000, False, pid)
        if not handle:
            return ctypes.get_last_error() == 5  # Access denied still means a process exists.
        try:
            code = wintypes.DWORD()
            return not kernel.GetExitCodeProcess(handle, ctypes.byref(code)) or code.value == 259
        finally:
            kernel.CloseHandle(handle)
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def _validated_discovery(path: Path) -> dict[str, Any]:
    try:
        if path.stat().st_size > 16 * 1024:
            raise ValueError("discovery file is too large")
        data = json.loads(path.read_text(encoding="utf-8-sig"))
        if not isinstance(data, dict) or data.get("protocol_version") != PROTOCOL_VERSION:
            raise ValueError("unsupported protocol version")
        url = data["base_url"]
        parsed = urllib.parse.urlsplit(url)
        if (parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
                or not parsed.port or parsed.username or parsed.password
                or parsed.query or parsed.fragment or parsed.path not in ("", "/")):
            raise ValueError("base_url must be a plain http://127.0.0.1:PORT URL")
        token = data["token"]
        if not isinstance(token, str) or not token or "\r" in token or "\n" in token:
            raise ValueError("invalid bearer token")
        if not isinstance(data.get("pid"), int) or data["pid"] <= 0:
            raise ValueError("invalid server PID")
        return {**data, "base_url": url.rstrip("/")}
    except (OSError, ValueError, TypeError, KeyError) as exc:
        # Do not include file contents or authentication values in errors.
        raise ClientError(f"Cannot use discovery file {path}: {type(exc).__name__}") from exc


class Client:
    def __init__(self, discovery: str | Path | None = None,
                 engine: str | Path | None = None, timeout: float = 180,
                 auto_start: bool = True, startup_timeout: float = 45):
        self.discovery = Path(discovery) if discovery else default_discovery()
        self.engine = Path(engine) if engine else default_engine()
        if timeout <= 0 or startup_timeout <= 0:
            raise ClientError("Timeouts must be positive.")
        self.timeout = timeout
        self.startup_timeout = startup_timeout
        self.auto_start = auto_start
        self._opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect())
        self._sequence = 0

    def _json(self, state: dict[str, Any], route: str, *, body: Any = None,
              authenticated: bool = True, timeout: float | None = None) -> Any:
        headers = {"Accept": "application/json"}
        if authenticated:
            headers["Authorization"] = "Bearer " + state["token"]
        payload = None
        if body is not None:
            payload = json.dumps(body, ensure_ascii=False, allow_nan=False).encode("utf-8")
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(state["base_url"] + route, data=payload, headers=headers,
                                         method="POST" if body is not None else "GET")
        try:
            with self._opener.open(request, timeout=timeout or self.timeout) as response:
                raw = response.read(MAX_RESPONSE_BYTES + 1)
                if len(raw) > MAX_RESPONSE_BYTES:
                    raise ClientError("API response exceeds the client size limit.")
                return json.loads(raw.decode("utf-8"))
        except urllib.error.HTTPError as exc:
            # A rejected HTTP request is not retried either. Never expose headers/token.
            raise HTTPFailure(exc.code) from exc

    def _find_healthy(self) -> dict[str, Any]:
        state = _validated_discovery(self.discovery)
        health = self._json(state, "/health", authenticated=False, timeout=2)
        if not isinstance(health, dict):
            raise ClientError("Unexpected health response.")
        if (health.get("service") != "touchline-editor" or health.get("ready") is not True
                or health.get("protocol_version") != PROTOCOL_VERSION or health.get("pid") != state["pid"]):
            raise ClientError("The discovery file belongs to a different API process.")
        return state

    def ensure_service(self) -> dict[str, Any]:
        try:
            return self._find_healthy()
        except (ClientError, OSError, ValueError, urllib.error.URLError):
            if not self.auto_start:
                raise ClientError("Touchline API is not available. Start it or omit --no-start.") from None
        try:
            existing = _validated_discovery(self.discovery)
        except ClientError:
            existing = None
        if existing and _process_exists(existing["pid"]):
            raise ClientError("The discovered API process is still running but is busy or not responding. "
                              "No second engine was started; wait for the current operation and retry the health check.")
        if not self.engine.is_file():
            raise ClientError(f"API-enabled engine not found: {self.engine}. Build it or pass --engine.")
        self.discovery.parent.mkdir(parents=True, exist_ok=True)
        lock = self.discovery.with_name(self.discovery.name + ".starting")
        owns_lock = False
        try:
            try:
                with lock.open("x", encoding="utf-8") as f:
                    json.dump({"pid": os.getpid(), "started": time.time()}, f)
                owns_lock = True
            except FileExistsError:
                # Another client is already starting the server. Wait for its discovery;
                # never kill a process or remove somebody else's startup lock.
                pass
            if owns_lock:
                # Recheck under the launch lock to avoid duplicate engines.
                try:
                    return self._find_healthy()
                except (ClientError, OSError, ValueError, urllib.error.URLError):
                    pass
                log_path = self.discovery.with_name("api-startup.log")
                with log_path.open("ab") as log:
                    subprocess.Popen(
                        [str(self.engine.resolve()), "--api", "--port", "0", "--discovery",
                         str(self.discovery.resolve())],
                        stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                        cwd=str(self.engine.resolve().parent),
                        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
                        close_fds=True,
                    )
            deadline = time.monotonic() + self.startup_timeout
            while time.monotonic() < deadline:
                try:
                    return self._find_healthy()
                except (ClientError, OSError, ValueError, urllib.error.URLError):
                    time.sleep(0.15)
            raise ClientError("API startup timed out. Inspect api-startup.log; no command was submitted. "
                              "A stale api.json.starting file can be removed after confirming no start is in progress.")
        finally:
            if owns_lock:
                lock.unlink(missing_ok=True)

    def health(self) -> dict[str, Any]:
        state = self.ensure_service()
        return self._json(state, "/health", authenticated=False)

    def capabilities(self) -> dict[str, Any]:
        state = self.ensure_service()
        return self._json(state, "/v1/capabilities")

    def shutdown(self) -> dict[str, Any]:
        # Explicit API shutdown never starts a new process just to stop it.
        state = self._find_healthy()
        try:
            return self._json(state, "/v1/shutdown", body={})
        except ClientError:
            raise
        except (OSError, ValueError, urllib.error.URLError) as exc:
            raise UncertainOutcome("No complete shutdown response; the request was not retried.") from exc

    def request(self, command: str, args: dict[str, Any] | None = None) -> dict[str, Any]:
        if (not isinstance(command, str) or not command
                or (args is not None and not isinstance(args, dict))):
            raise ClientError("A command name and JSON object arguments are required.")
        arguments = {} if args is None else args
        # Validate locally before any command can be submitted.
        json.dumps(arguments, ensure_ascii=False, allow_nan=False)
        state = self.ensure_service()
        self._sequence += 1
        request = {"id": self._sequence, "command": command, "args": arguments}
        try:
            result = self._json(state, "/v1/command", body=request)
        except (HTTPFailure, ClientError, OSError, ValueError, urllib.error.URLError) as exc:
            if isinstance(exc, HTTPFailure) and exc.code < 500:
                raise
            raise UncertainOutcome(
                f"No complete response for '{command}'. Its outcome is unknown; it was not retried. "
                "For edits inspect history / batch_history / relation_history / event_history before acting again."
            ) from exc
        if (not isinstance(result, dict) or result.get("id") != request["id"]
                or not isinstance(result.get("ok"), bool)):
            raise UncertainOutcome(f"Invalid response for '{command}'; outcome unknown. Inspect history; do not replay the edit.")
        return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Use the persistent, local Touchline editor API.")
    parser.add_argument("command", help="Bridge command, or health / capabilities / start / stop")
    args_group = parser.add_mutually_exclusive_group()
    args_group.add_argument("--args", default=None, help="JSON object of command arguments")
    args_group.add_argument("--args-file", type=Path, help="UTF-8 JSON argument file (recommended on PowerShell)")
    parser.add_argument("--discovery", type=Path)
    parser.add_argument("--engine", type=Path)
    parser.add_argument("--timeout", type=float, default=180)
    parser.add_argument("--startup-timeout", type=float, default=45)
    parser.add_argument("--no-start", action="store_true", help="Fail instead of starting an API engine")
    parser.add_argument("--output", type=Path, help="Also write the response as UTF-8 JSON")
    ns = parser.parse_args(argv)
    try:
        arguments = json.loads(ns.args_file.read_text(encoding="utf-8-sig")) if ns.args_file else json.loads(ns.args or "{}")
        if not isinstance(arguments, dict):
            raise ClientError("Command arguments must be a JSON object.")
        client = Client(ns.discovery, ns.engine, ns.timeout, not ns.no_start, ns.startup_timeout)
        if ns.command in ("health", "start"):
            result = client.health()
        elif ns.command == "capabilities":
            result = client.capabilities()
        elif ns.command == "stop":
            result = client.shutdown()
        else:
            result = client.request(ns.command, arguments)
        text = json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False)
        if ns.output:
            ns.output.parent.mkdir(parents=True, exist_ok=True)
            ns.output.write_text(text + "\n", encoding="utf-8")
        print(text)
        return 0 if result.get("ok", True) else 1
    except (ClientError, OSError, ValueError) as exc:
        print(str(exc), file=sys.stderr)
        return 2


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stderr.reconfigure(encoding="utf-8")
    raise SystemExit(main())
