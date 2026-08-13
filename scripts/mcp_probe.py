#!/usr/bin/env python3
"""Drive a telegram-connector MCP stdio server and time tools/call round trips.

Latency probe for work that needs real-world timings against a live
authenticated session — the one thing the offline test suite cannot exercise.
Used to produce the measurements in
`docs/superpowers/specs/2026-08-13-global-search-latency-design.md`.

Usage:
    scripts/mcp_probe.py <binary> <calls.json>
    scripts/mcp_probe.py /Applications/telegram-mcp scripts/probes/search-latency.json

calls.json is a list of {"label": str, "name": str, "arguments": {...}} objects.
A call named "__tools_list__" issues tools/list instead of tools/call.
Emits one JSON object per line on stdout: {"label", "elapsed_s", "ok", "result"}.
Server stderr is teed to the path in PROBE_STDERR (default: probe-stderr.log).

Note: every call spends real rate-limiter tokens against a live Telegram
account (searches cost 1, media and transcription cost more — see
`[rate_limiting]`). Probes are cheap but not free; keep call lists short.

Time windows expressed as `hours_back` are stable across runs. Probes using
absolute `from_date`/`to_date` need those recomputed relative to now, so they
are not checked in.
"""

import json
import os
import subprocess
import sys
import time


def main():
    binary, calls_path = sys.argv[1], sys.argv[2]
    calls = json.load(open(calls_path))
    stderr_path = os.environ.get("PROBE_STDERR", "probe-stderr.log")

    with open(stderr_path, "wb") as errf:
        proc = subprocess.Popen(
            [binary],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=errf,
            text=True,
            bufsize=1,
        )

        def send(obj):
            proc.stdin.write(json.dumps(obj) + "\n")
            proc.stdin.flush()

        def read_response(expect_id):
            """Read lines until a JSON-RPC response with the expected id."""
            while True:
                line = proc.stdout.readline()
                if not line:
                    raise RuntimeError("server closed stdout")
                line = line.strip()
                if not line:
                    continue
                try:
                    msg = json.loads(line)
                except json.JSONDecodeError:
                    continue  # stray non-JSON on stdout
                if msg.get("id") == expect_id:
                    return msg

        # Legacy initialize handshake (rmcp falls back to it automatically).
        send({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "latency-probe", "version": "0.1.0"},
            },
        })
        init = read_response(1)
        print(json.dumps({"label": "__initialize__",
                          "ok": "result" in init,
                          "server": init.get("result", {}).get("serverInfo")}),
              flush=True)
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})

        next_id = 2
        for call in calls:
            rid = next_id
            next_id += 1
            if call["name"] == "__tools_list__":
                req = {"jsonrpc": "2.0", "id": rid, "method": "tools/list", "params": {}}
            else:
                req = {
                    "jsonrpc": "2.0", "id": rid, "method": "tools/call",
                    "params": {"name": call["name"], "arguments": call.get("arguments", {})},
                }
            t0 = time.monotonic()
            send(req)
            resp = read_response(rid)
            elapsed = time.monotonic() - t0
            print(json.dumps({
                "label": call["label"],
                "elapsed_s": round(elapsed, 3),
                "ok": "result" in resp,
                "result": resp.get("result", resp.get("error")),
            }), flush=True)

        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.terminate()


if __name__ == "__main__":
    main()
