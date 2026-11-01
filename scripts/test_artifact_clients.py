#!/usr/bin/env python3
"""Independent NDJSON checks for opt-in processed JPEG artifacts; no credentials."""
import base64
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from test_media_clients import body, send


@contextmanager
def fixture(binary, modern, ttl=600, canonical=True, enabled=True):
    with tempfile.TemporaryDirectory(prefix="telegram-artifact-client-") as temp:
        root = Path(temp).resolve() / "artifacts"
        args = [binary, "--typed-tools" if canonical else "--legacy-typed-tools"]
        if enabled:
            args += ["--artifact-root", str(root), "--artifact-ttl-seconds", str(ttl)]
        proc = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE)
        try:
            if not modern:
                initialized = send(proc, 0, "initialize", {
                    "protocolVersion": "2025-11-25", "capabilities": {},
                    "clientInfo": {"name": "python-artifact-client", "version": "1"}}, False)
                assert initialized["result"]["protocolVersion"] == "2025-11-25"
                proc.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
                proc.stdin.flush()
            yield proc, root
            proc.stdin.close()
            proc.wait(timeout=5)
            assert proc.returncode == 0, proc.stderr.read().decode()
            assert not root.exists() or not list(root.iterdir()), "ordinary EOF left owned artifacts"
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait(timeout=3)
            for stream in (proc.stdin, proc.stdout, proc.stderr):
                stream.close()


def call(proc, modern, delivery, batch=False):
    arguments = {"channel_id": "100", "delivery": delivery}
    arguments.update({"message_ids": [1, 2, 3]} if batch else {"message_id": 1})
    return send(proc, 10, "tools/call", {
        "name": "get_messages_media_batch" if batch else "get_message_media",
        "arguments": arguments}, modern)


def hints(result, modern, ttl):
    if modern:
        assert result["cacheScope"] == "private"
        assert 0 <= result["ttlMs"] <= ttl * 1000
    else:
        assert all(key not in result for key in ("cacheScope", "ttlMs", "resultType"))


def error(reply, discriminator, numeric):
    assert reply["error"]["data"]["code"] == discriminator, reply
    assert reply["error"]["code"] == numeric, reply


def check_modes(binary, modern):
    with fixture(binary, modern) as (proc, root):
        tools = send(proc, 1, "tools/list", {}, modern)["result"]["tools"]
        assert len(tools) == 16
        for method, key in (("resources/list", "resources"),
                            ("resources/templates/list", "resourceTemplates")):
            reply = send(proc, 2, method, {}, modern)
            assert "result" in reply, f"explicit artifact opt-in must enable {method}: {reply}"
            listing = reply["result"]
            assert listing[key] == []
            hints(listing, modern, 0)
        for delivery in ("inline", "resource", "file", "resource_and_file"):
            for batch in (False, True):
                reply = call(proc, modern, delivery, batch)
                data = body(reply, True)
                blocks = reply["result"]["content"]
                assert data["summary"]["returned"] == (2 if batch else 1)
                assert data["summary"]["failed"] == (1 if batch else 0)
                cursor = 1
                for position, item in enumerate(data["items"]):
                    if batch and position == 1:
                        assert item["error"]["code"] == "message_not_found"
                        assert "content_index" not in item and "artifact" not in item
                        continue
                    assert item["content_index"] == cursor
                    preview = blocks[cursor]
                    assert preview["type"] == "image" and preview["mimeType"] == "image/jpeg"
                    jpeg = base64.b64decode(preview["data"], validate=True)
                    assert jpeg.startswith(b"\xff\xd8") and jpeg.endswith(b"\xff\xd9")
                    cursor += 1
                    if delivery == "inline":
                        assert "artifact" not in item
                        continue
                    artifact = item["artifact"]
                    assert artifact["kind"] == "processed_jpeg"
                    assert artifact["mime_type"] == "image/jpeg"
                    assert artifact["width"] == artifact["height"] == 64
                    assert artifact["size_bytes"] == len(jpeg)
                    assert datetime.fromisoformat(artifact["expires_at"].replace("Z", "+00:00")) > datetime.now(timezone.utc)
                    if "resource" in delivery:
                        uri = artifact["resource_uri"]
                        assert re.fullmatch(r"telegram-media://artifact/[0-9a-f]{48}", uri)
                        assert artifact["resource_content_index"] == cursor
                        assert blocks[cursor]["type"] == "resource_link"
                        assert blocks[cursor]["uri"] == uri
                        cursor += 1
                        read = send(proc, 3, "resources/read", {"uri": uri}, modern)["result"]
                        hints(read, modern, 600)
                        assert read["contents"][0]["uri"] == uri
                        assert read["contents"][0]["mimeType"] == "image/jpeg"
                        assert base64.b64decode(read["contents"][0]["blob"], validate=True) == jpeg
                    else:
                        assert "resource_uri" not in artifact and "resource_content_index" not in artifact
                    if "file" in delivery:
                        path = Path(artifact["file_path"])
                        assert path.is_absolute() and path.parent == root
                        assert path.read_bytes() == jpeg
                        assert path.stat().st_mode & 0o777 == 0o600
                    else:
                        assert "file_path" not in artifact
                assert len(blocks) == cursor
        for uri, discriminator, numeric in (
                ("file:///private/secret", "artifact_invalid_uri", -32602),
                ("telegram-media://artifact/" + "0" * 48, "artifact_missing", -32602 if modern else -32002)):
            reply = send(proc, 4, "resources/read", {"uri": uri}, modern)
            error(reply, discriminator, numeric)
            assert "/private/secret" not in str(reply)
    print(f"artifact modes/bytes/indices/errors/cache hints/EOF cleanup: {'modern' if modern else 'legacy'} PASS")


def check_expiry_storage(binary, modern):
    with fixture(binary, modern, ttl=1) as (proc, _):
        item = body(call(proc, modern, "resource_and_file"), True)["items"][0]
        time.sleep(1.1)
        reply = send(proc, 20, "resources/read", {"uri": item["artifact"]["resource_uri"]}, modern)
        error(reply, "artifact_missing", -32602 if modern else -32002)
    with fixture(binary, modern) as (proc, _):
        artifact = body(call(proc, modern, "resource_and_file"), True)["items"][0]["artifact"]
        path = Path(artifact["file_path"])
        original = path.read_bytes()
        path.write_bytes(b"truncated")
        reply = send(proc, 21, "resources/read", {"uri": artifact["resource_uri"]}, modern)
        error(reply, "artifact_unavailable", -32603)
        assert str(path) not in str(reply)
        path.write_bytes(original)
    print(f"artifact expiry and tampered-storage refusal: {'modern' if modern else 'legacy'} PASS")


def check_unavailable(binary, modern):
    for canonical, enabled in ((True, False), (False, True)):
        with fixture(binary, modern, canonical=canonical, enabled=enabled) as (proc, root):
            reply = call(proc, modern, "resource")
            assert reply["result"]["isError"] is True, reply
            if canonical:
                assert reply["result"]["structuredContent"]["error"]["code"] == "invalid_input"
            assert not root.exists() or len(list(root.glob("*.jpg"))) == 0
    print(f"artifact disabled/legacy-output rejection: {'modern' if modern else 'legacy'} PASS")


if __name__ == "__main__":
    for modern_protocol in (True, False):
        check_modes(sys.argv[1], modern_protocol)
        check_expiry_storage(sys.argv[1], modern_protocol)
        check_unavailable(sys.argv[1], modern_protocol)
    print("Artifact client matrix PASS (oversize/read-timeout boundaries remain direct Rust tests)")
