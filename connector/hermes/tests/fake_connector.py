"""A stand-in for `trommi-connector` (MCP over stdio, as the add-on speaks to it): answers initialize and tools/call,
notes every notification in FAKE_LOG, and once initialized says one board event."""
import json
import os
import sys

log = open(os.environ["FAKE_LOG"], "a")


def out(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    msg = json.loads(line)
    method, rid = msg.get("method"), msg.get("id")
    if rid is not None and method == "initialize":
        out({"jsonrpc": "2.0", "id": rid, "result": {"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "trommi"}}})
    elif rid is not None and method == "tools/call":
        name = msg["params"]["name"]
        out({"jsonrpc": "2.0", "id": rid, "result": {"content": [{"type": "text", "text": f"{name} in {os.environ['TROMMI_SESSION_KEY']}"}]}})
    elif rid is None:
        log.write(json.dumps({"method": method, "params": msg.get("params")}) + "\n")
        log.flush()
        if method == "notifications/initialized":
            out({"jsonrpc": "2.0", "method": "notifications/claude/channel", "params": {"content": "Hallo", "meta": {"kind": "chat"}}})
