#!/usr/bin/env python3
"""Offline process lifecycle checks; pass the offline_mcp_fixture executable."""
import json
import os
import selectors
import signal
import subprocess
import sys
import time

META = {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}}

def send(proc, rid, method, **params):
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method,
                                "params": {"_meta": META, **params}}).encode() + b"\n")
    proc.stdin.flush()

def read(proc):
    with selectors.DefaultSelector() as poll:
        poll.register(proc.stdout, selectors.EVENT_READ)
        assert poll.select(3), "wire response deadline"
    return json.loads(proc.stdout.readline())

def check(binary, pending, signal_shutdown):
    proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE)
    try:
        send(proc, 1, "tools/list")
        assert read(proc)["result"]["resultType"] == "complete"
        if pending:
            send(proc, 2, "tools/call", name="wait", arguments={},
                 _meta={**META, "progressToken": "fixture-wait"})
            assert read(proc)["method"] == "notifications/progress"
        start = time.monotonic()
        if signal_shutdown:
            # Leave stdin open to exercise the blocking-stdin shutdown limitation.
            proc.send_signal(signal.SIGTERM)
        else:
            proc.stdin.close()
            proc.stdin = None
        proc.wait(timeout=2.5)
        assert proc.returncode == 0, proc.stderr.read().decode()
        assert time.monotonic() - start < 2.5
    finally:
        if proc.poll() is None:
            proc.kill()
        proc.communicate(timeout=3)

def check_saturated_bootstrap(binary):
    # Fill the child's output pipe before launch and never drain it. This makes
    # even a small discovery response block independently of OS pipe capacity.
    output_read, output_write = os.pipe()
    proc = None
    try:
        os.set_blocking(output_write, False)
        try:
            while True:
                os.write(output_write, b"x" * 4096)
        except BlockingIOError:
            pass
        os.set_blocking(output_write, True)
        proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=output_write,
                                stderr=subprocess.PIPE)
        request = {"jsonrpc": "2.0", "id": 1, "method": "server/discover",
                   "params": {"_meta": META}}
        data = json.dumps(request).encode() + b"\n" + b" " * 32768
        # communicate bounds the producer too: stdin may itself back up once
        # the bounded pump fills. stdout is our unread pipe, not proc.stdout.
        proc.communicate(input=data, timeout=2.5)
        assert proc.returncode == 0, "bootstrap stall must terminate cleanly"
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
            proc.communicate(timeout=3)
        os.close(output_read)
        os.close(output_write)


if __name__ == "__main__":
    for pending in (False, True):
        for signal_shutdown in (False, True):
            check(sys.argv[1], pending, signal_shutdown)
    check_saturated_bootstrap(sys.argv[1])
    print("offline process lifecycle: idle/pending EOF and signal shutdown plus saturated bootstrap passed")
