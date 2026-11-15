#!/usr/bin/env python3
"""Verify both probe modes against the credential-free fixture executable."""
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from mcp_probe import Responses

with tempfile.TemporaryDirectory() as directory:
    calls = Path(directory) / "calls.json"
    calls.write_text(json.dumps([{"label": "list", "name": "__tools_list__"},
                                 {"label": "echo", "name": "echo"}]))
    for protocol, flags in (("modern", []), ("modern", ["--skip-discovery"]), ("legacy", [])):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("mcp_probe.py")),
                                 "--protocol", protocol, *flags, sys.argv[1], str(calls)],
                                env={**os.environ, "PROBE_STDERR": str(Path(directory) / "stderr.log")},
                                capture_output=True, text=True, timeout=10)
        assert result.returncode == 0, result.stderr
        lines = [json.loads(line) for line in result.stdout.splitlines()]
        assert all(line["ok"] for line in lines), lines
        assert lines[-1]["result"]["content"][0]["text"] == "offline fixture", lines
    print("offline probe: modern and legacy modes passed")

# The real sending routes, source policy, staging reader and independent process
# transport run with the inert client: no credentials or Telegram calls.
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    prepared, staging = root / "prepared", root / "staging"
    prepared.mkdir(mode=0o700)
    staging.mkdir(mode=0o700)
    proc = subprocess.Popen([sys.argv[1], "--typed-tools", "--sending-enabled", "true",
                             "--upload-root", str(prepared), "--staging-root", str(staging)],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    responses = Responses(proc.stdout, 10)
    def request(rid, name, arguments):
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": "tools/call",
            "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
                                    "io.modelcontextprotocol/clientCapabilities": {}},
                       "name": name, "arguments": arguments}}).encode() + b"\n")
        proc.stdin.flush()
        return responses.read(rid)
    try:
        preview = request(1, "get_message_media", {"channel_id": "100", "message_id": 1})
        index = preview["result"]["structuredContent"]["data"]["items"][0]["content_index"]
        jpeg = base64.b64decode(preview["result"]["content"][index]["data"], validate=True)
        assert jpeg.startswith(b"\xff\xd8")
        jpeg = jpeg[:2] + b"\xff\xe1\xff\xff" + bytes(65533) + b"\xff\xfe\xff\xff" + bytes(65533) + jpeg[2:]
        mp3 = (Path(__file__).resolve().parents[1] / "src/sending/tests/fixtures/tone.mp3").read_bytes()
        # 131072 padding bytes, encoded as the ID3 synchsafe size 00 08 00 00.
        mp3 = b"ID3\x04\0\0\0\x08\0\0" + bytes(131072) + mp3
        for rid, data, media, mime in [
            (2, jpeg, {"kind": "image"}, "image/jpeg"),
            (3, mp3, {"kind": "audio", "duration_seconds": 1}, "audio/mpeg"),
        ]:
            path = f"metadata-{rid}.bin"
            (prepared / path).write_bytes(data)
            reply = request(rid, "send_media", {"recipient": {"kind": "saved_messages"},
                "source": {"root": "outgoing", "relative_path": path}, "media": media})
            assert reply["result"].get("isError") is not True, reply
            body = reply["result"]["structuredContent"]
            assert json.loads(reply["result"]["content"][0]["text"]) == body
            assert body["data"]["delivery_state"] == "sent", body
            assert body["data"]["media"] == {"mime_type": mime, "size_bytes": len(data)}, body
        proc.stdin.close()
        proc.wait(timeout=10)
        assert proc.returncode == 0, proc.stderr.read().decode()
        assert not list(staging.iterdir()), "snapshot leases must be released"
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=3)
        proc.stdout.close()
        proc.stderr.close()
    print("offline sending probe: genuine JPEG/MP3 after large metadata passed")
