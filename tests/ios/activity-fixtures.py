#!/usr/bin/env python3
"""Write isolated native Hermes activity fixtures; never contacts a Hub.

python tests/ios/activity-fixtures.py /absolute/output/activity
Then set TROMMI_SPEED_FIXTURE to that prefix for
NativeMessagesAcceptanceTests/testHermesAutomaticWorkAndIdleWithManualTask.
"""
import copy
import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2]
base = json.loads((root / "demo/data/fixture.json").read_text())
prefix = Path(sys.argv[1])
prefix.parent.mkdir(parents=True, exist_ok=True)
sid = base["sessions"][0]["agent_device_id"]
for state in ("work", "idle-manual", "final"):
    fixture = copy.deepcopy(base)
    session = fixture["sessions"][0]
    session["link"] = {"hears": "live", "attached": True, "working": state == "work"}
    session["profile"].update(agent_name="Hermes Activity Fixture", model="Hermes")
    session["status_lines"] = [] if state == "work" else [{
        "id": "fixture-manual", "label": "Manual task remains", "state": "working",
        "detail": "Independent of this turn", "updated_at": fixture["made_at"] - 1000,
    }]
    for member in fixture["members"]:
        if member["device_id"] == sid:
            member["platform"] = "hermes"
    fixture["cards"] = []
    fixture["permissions"] = []
    timeline = "chat:session/" + sid[:32]
    fixture["timelines"] = {timeline: []}
    if state == "final":
        row = copy.deepcopy(next(iter(base["timelines"].values()))[0])
        row.update(sender_device_id=sid, sent_at=fixture["made_at"] - 1000,
                   envelope_number=999991,
                   content={"text": "The isolated answer is finished.", "attachments": []})
        fixture["timelines"][timeline] = [row]
    Path(str(prefix) + "-" + state + ".json").write_text(json.dumps(fixture))
print("Wrote three isolated activity fixtures")
