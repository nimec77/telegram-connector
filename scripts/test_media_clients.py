#!/usr/bin/env python3
"""Exercise real typed routes from an independent credential-free Python NDJSON client."""
import base64
import json
from pathlib import Path
import selectors
import subprocess
import sys

META = {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}}


def read(proc):
    with selectors.DefaultSelector() as poll:
        poll.register(proc.stdout, selectors.EVENT_READ)
        assert poll.select(3), "wire response deadline"
    return json.loads(proc.stdout.readline())


def send(proc, rid, method, params, modern):
    params = dict(params)
    if modern:
        params["_meta"] = META
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid,
                                "method": method, "params": params}).encode() + b"\n")
    proc.stdin.flush()
    return read(proc)


def body(reply, canonical):
    result = reply["result"]
    text = json.loads(result["content"][0]["text"])
    if canonical:
        assert text == result["structuredContent"]
        assert text["schema_version"] == "1" and text["error"] is None
        return text["data"]
    return text


def check(binary, canonical, modern):
    flag = "--typed-tools" if canonical else "--legacy-typed-tools"
    proc = subprocess.Popen([binary, flag], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        if not modern:
            reply = send(proc, 0, "initialize", {"protocolVersion": "2025-11-25",
                         "capabilities": {}, "clientInfo": {"name": "python-media-client", "version": "1"}}, False)
            assert reply["result"]["protocolVersion"] == "2025-11-25"
            proc.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
            proc.stdin.flush()
        listing = send(proc, 1, "tools/list", {}, modern)["result"]["tools"]
        assert len(listing) == 16, listing
        original = send(proc, 2, "tools/call", {"name": "get_message_media", "arguments": {
            "channel_id": "100", "message_id": 1, "recovery_key": "python-image"}}, modern)
        assert original["result"].get("isError") is not True
        index = body(original, True)["items"][0]["content_index"] if canonical else 0
        image = original["result"]["content"][index]
        assert image["type"] == "image" and image["mimeType"] == "image/jpeg"
        jpeg = base64.b64decode(image["data"], validate=True)
        assert jpeg.startswith(b"\xff\xd8") and jpeg.endswith(b"\xff\xd9")
        if canonical:
            metadata = body(original, True)["items"][0]["metadata"]
            assert metadata["returned_width"] == metadata["returned_height"] == 64
        Path("/private/tmp/telegram-task8-preview.jpg").write_bytes(jpeg)
        for rid, include_binary in [(3, False), (4, True)]:
            recovered = send(proc, rid, "tools/call", {"name": "get_last_responses", "arguments": {
                "recovery_keys": ["python-image"], "include_binary": include_binary}}, modern)
            selection = body(recovered, canonical)["responses"]
            assert len(selection) == 1 and selection[0]["recovery_key"] == "python-image"
            block = selection[0]["response"]["result"]["content"][index]
            if include_binary:
                assert block["data"] == image["data"]
            else:
                assert block["omitted"] is True and "data" not in block
        batch = send(proc, 5, "tools/call", {"name": "get_messages_media_batch", "arguments": {
            "channel_id": "100", "message_ids": [1, 2, 3]}}, modern)
        assert batch["result"].get("isError") is not True
        if canonical:
            data = body(batch, True)
            assert data["summary"]["returned"] == 2 and data["summary"]["failed"] == 1
            for item, expected in zip(data["items"], [1, None, 2]):
                if expected is None:
                    assert "content_index" not in item and item["error"]["code"] == "message_not_found"
                else:
                    assert item["content_index"] == expected
                    assert batch["result"]["content"][expected]["type"] == "image"
        else:
            summary = json.loads(batch["result"]["content"][-1]["text"])
            assert summary["returned"] == 2 and summary["failed"][0]["id"] == 2
        proc.stdin.close()
        proc.wait(timeout=3)
        assert proc.returncode == 0, proc.stderr.read().decode()
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=3)
        proc.stdout.close()
        proc.stderr.close()


if __name__ == "__main__":
    for canonical_mode in (True, False):
        for modern_protocol in (True, False):
            check(sys.argv[1], canonical_mode, modern_protocol)
    print("Python media client: typed/legacy output across modern/legacy protocol, previews, keyed replay and mixed batches passed")
