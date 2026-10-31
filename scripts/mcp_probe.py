#!/usr/bin/env python3
"""Time explicit modern or legacy MCP calls over stdio.

Usage: mcp_probe.py [--protocol modern|legacy] [--skip-discovery] binary calls.json
calls.json is a list of {label, name, arguments}; __tools_list__ lists tools.
A real connector requires authentication and tool calls can spend account tokens.
For offline checks use the offline_mcp_fixture example. Server stderr is captured
in PROBE_STDERR (default probe-stderr.log). Stdout contains only probe result JSON.
"""
import argparse
import json
import os
import selectors
import subprocess
import time


class Responses:
    """Bounded-time reads without relying on a buffered stream's readiness state."""
    def __init__(self, stream, timeout):
        self.stream = stream
        self.timeout = timeout
        self.pending = bytearray()

    def read(self, expected_id):
        deadline = time.monotonic() + self.timeout
        with selectors.DefaultSelector() as poll:
            poll.register(self.stream, selectors.EVENT_READ)
            while True:
                if b"\n" in self.pending:
                    line, _, rest = self.pending.partition(b"\n")
                    self.pending = bytearray(rest)
                    if not line.strip():
                        continue
                    message = json.loads(line)  # Stray stdout is a protocol failure.
                    if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
                        raise RuntimeError("invalid JSON-RPC output")
                    if "id" not in message:
                        if "method" not in message:
                            raise RuntimeError("uncorrelated protocol error")
                        continue
                    if message["id"] != expected_id or "method" in message:
                        raise RuntimeError("unexpected response ID or server request")
                    return message
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not poll.select(remaining):
                    raise TimeoutError("MCP response deadline expired")
                data = os.read(self.stream.fileno(), 65536)
                if not data:
                    raise RuntimeError("server closed stdout")
                self.pending.extend(data)
                if len(self.pending) > 32 * 1024 * 1024:
                    raise RuntimeError("probe response exceeded 32 MiB")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--protocol", choices=("modern", "legacy"), default="modern")
    parser.add_argument("--skip-discovery", action="store_true")
    parser.add_argument("--timeout", type=float, default=180)
    parser.add_argument("--shutdown-timeout", type=float, default=10)
    parser.add_argument("binary")
    parser.add_argument("calls")
    args = parser.parse_args()
    if args.timeout <= 0 or args.shutdown_timeout <= 0:
        parser.error("timeouts must be positive")
    if args.skip_discovery and args.protocol != "modern":
        parser.error("--skip-discovery requires modern protocol")
    with open(args.calls, encoding="utf-8") as file:
        calls = json.load(file)
    meta = {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {"name": "latency-probe", "version": "0.2.0"}}
    with open(os.environ.get("PROBE_STDERR", "probe-stderr.log"), "wb") as errf:
        proc = subprocess.Popen([args.binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=errf)
        responses = Responses(proc.stdout, args.timeout)

        def send(message):
            proc.stdin.write(json.dumps(message).encode() + b"\n")
            proc.stdin.flush()

        def request(rid, method, params):
            if args.protocol == "modern":
                params = {**params, "_meta": meta}
            send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
            return responses.read(rid)

        def output(label, elapsed, response):
            result = response.get("result", response.get("error"))
            ok = "result" in response and not response["result"].get("isError", False)
            print(json.dumps({"label": label, "elapsed_s": round(elapsed, 3),
                              "ok": ok, "result": result}), flush=True)
            return ok

        try:
            rid = 1
            if args.protocol == "legacy":
                response = request(rid, "initialize", {"protocolVersion": "2025-11-25",
                    "capabilities": {}, "clientInfo": {"name": "latency-probe", "version": "0.2.0"}})
                if not output("__initialize__", 0, response):
                    raise RuntimeError("initialize failed")
                if response["result"]["protocolVersion"] != "2025-11-25":
                    raise RuntimeError("server did not negotiate the requested legacy revision")
                send({"jsonrpc": "2.0", "method": "notifications/initialized"})
                rid += 1
            elif not args.skip_discovery:
                response = request(rid, "server/discover", {})
                if not output("__discover__", 0, response):
                    raise RuntimeError("modern discovery failed")
                if "2026-07-28" not in response["result"]["supportedVersions"]:
                    raise RuntimeError("server does not support the requested modern revision")
                rid += 1
            for call in calls:
                method = "tools/list" if call["name"] == "__tools_list__" else "tools/call"
                params = {} if method == "tools/list" else {"name": call["name"], "arguments": call.get("arguments", {})}
                started = time.monotonic()
                response = request(rid, method, params)
                output(call["label"], time.monotonic() - started, response)
                rid += 1
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=args.shutdown_timeout)
            except subprocess.TimeoutExpired:
                proc.terminate()
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=2)
            proc.stdout.close()


if __name__ == "__main__":
    main()
