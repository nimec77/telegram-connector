#!/usr/bin/env python3
"""Verify both probe modes against the credential-free fixture executable."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

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
