from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

from trommi_platform.child import Child

FAKE = Path(__file__).with_name("fake_connector.py")


def fake_program(tmp_path: Path) -> str:
    program = tmp_path / "trommi-connector"
    program.write_text(f"#!/bin/sh\nexec '{sys.executable}' '{FAKE}'\n")
    program.chmod(0o755)
    return str(program)


async def test_a_child_speaks_mcp_and_hands_on_what_the_connector_says(tmp_path):
    log = tmp_path / "log"
    heard: list[tuple[str, dict]] = []

    async def notice(method, params):
        heard.append((method, params))

    child = Child(fake_program(tmp_path), {"FAKE_LOG": str(log), "TROMMI_SESSION_KEY": "hermes-x"}, notice, name="main")
    await child.start()
    assert await child.call_tool("create_info", {"title": "x"}) == "create_info in hermes-x"
    await child.notify("trommi/answer", {"text": "Sonnig"})
    for _ in range(100):
        if heard:
            break
        await asyncio.sleep(0.05)
    assert heard == [("notifications/claude/channel", {"content": "Hallo", "meta": {"kind": "chat"}})]
    await child.stop()
    notes = [json.loads(line) for line in log.read_text().splitlines()]
    assert {"method": "trommi/answer", "params": {"text": "Sonnig"}} in notes
    assert notes[0]["method"] == "notifications/initialized"
