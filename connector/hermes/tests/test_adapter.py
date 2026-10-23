from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest
from gateway.config import Platform, PlatformConfig

from trommi_platform import adapter as adapter_mod
from trommi_platform import plugin
from trommi_platform.adapter import TrommiAdapter
from trommi_platform.chats import MAIN, Chats


class FakeChild:
    made: list["FakeChild"] = []

    def __init__(self, program, env, on_notice, *, name):
        self.env, self.on_notice, self.name = env, on_notice, name
        self.notes: list[tuple[str, dict]] = []
        self.calls: list[tuple[str, dict]] = []
        self.started = self.stopped = False
        FakeChild.made.append(self)

    async def start(self):
        self.started = True

    async def stop(self):
        self.stopped = True

    async def notify(self, method, params):
        self.notes.append((method, params))

    async def call_tool(self, name, args):
        self.calls.append((name, args))
        return f"{name} in {self.env['TROMMI_SESSION_KEY']}"


@pytest.fixture
def made():
    FakeChild.made = []
    return FakeChild.made


def adapter(tmp_path: Path, instance: str = "Einkauf") -> TrommiAdapter:
    chats = Chats(program="trommi-connector", instance=instance, name=f"Hermes {instance}", folder=str(tmp_path),
                  state_file=tmp_path / "state" / "trommi.json", on_event=None, make_child=FakeChild)
    cfg = PlatformConfig(enabled=True, extra={"instance": instance.lower()})
    return TrommiAdapter(cfg, chats=chats, platform=Platform("trommi"))


async def test_a_board_message_is_a_message_of_that_chat(tmp_path, made, monkeypatch):
    a = adapter(tmp_path)
    got = []

    async def handle(event):
        got.append(event)

    monkeypatch.setattr(a, "handle_message", handle)
    assert await a.connect()
    main = made[0]
    assert (main.name, main.env["TROMMI_SESSION_KEY"], main.env["TROMMI_HOST"], main.env["TROMMI_CHANNEL_EVENTS"]) == ("main", "hermes-einkauf", "hermes", "on")
    await main.on_notice("notifications/claude/channel", {"content": "Wie ist das Wetter?", "meta": {"kind": "chat"}})
    assert [(e.text, e.source.chat_id, str(e.source.platform.value)) for e in got] == [("Wie ist das Wetter?", "main", "trommi")]
    await a.disconnect()
    assert main.stopped


async def test_the_answer_its_live_text_and_working_go_to_their_chat(tmp_path, made):
    a = adapter(tmp_path)
    await a.connect()
    main = made[0]
    assert (await a.send("main", "Sonnig, 21 Grad.")).success
    assert (await a.send_draft("main", 7, "Sonn")).success
    await a.send_typing("main")
    await a.stop_typing("main")
    assert main.notes == [("trommi/answer", {"text": "Sonnig, 21 Grad."}), ("trommi/preview", {"draft": "7", "text": "Sonn"}),
                          ("trommi/working", {"on": True}), ("trommi/working", {"on": False})]
    assert not (await a.send("c-000000000000", "x")).success, "no such chat: nothing goes"


async def test_an_approval_is_a_card_and_its_verdict_resolves_that_request(tmp_path, made, monkeypatch):
    a = adapter(tmp_path)
    await a.connect()
    main = made[0]
    monkeypatch.setattr(adapter_mod, "_pending_request_id", lambda key: "req-7")
    resolved = []
    import tools.approval

    monkeypatch.setattr(tools.approval, "resolve_gateway_approval", lambda key, choice, request_id=None: resolved.append((key, choice, request_id)) or 1)
    prompt = SimpleNamespace(chat_id="main", session_key="agent:main:trommi:dm:main", command="rm -rf build", description="dangerous")
    assert (await a._send_exec_approval_prompt(prompt)).success
    method, params = main.notes[-1]
    assert method == "notifications/claude/channel/permission_request"
    assert (params["tool_name"], params["input_preview"]) == ("Hermes", "rm -rf build")
    await main.on_notice("notifications/claude/channel/permission", {"request_id": params["request_id"], "behavior": "allow"})
    assert resolved == [("agent:main:trommi:dm:main", "once", "req-7")]
    # a verdict nobody asked for changes nothing
    await main.on_notice("notifications/claude/channel/permission", {"request_id": "other", "behavior": "allow"})
    assert len(resolved) == 1


async def test_the_person_asks_for_a_new_chat_once(tmp_path, made):
    a = adapter(tmp_path)
    await a.connect()
    main = made[0]
    await main.on_notice("notifications/claude/channel", {"content": "x", "meta": {"kind": "new_chat", "nonce": "n1"}})
    assert len(made) == 2
    new = made[1]
    assert new.name.startswith("c-") and new.started
    assert new.env["TROMMI_SESSION_KEY"] == f"hermes-einkauf-{new.name}"
    assert new.env["TROMMI_AGENT_NAME"] == "Hermes Einkauf 2"
    assert new.calls == [("connect", {})], "it activates itself on the approved computer"
    # the same ask again, or one from another chat: no second chat
    await main.on_notice("notifications/claude/channel", {"content": "x", "meta": {"kind": "new_chat", "nonce": "n1"}})
    await new.on_notice("notifications/claude/channel", {"content": "x", "meta": {"kind": "new_chat", "nonce": "n2"}})
    assert len(made) == 2
    # kept: the next start brings both back
    await a.disconnect()
    b = adapter(tmp_path)
    await b.connect()
    assert sorted(c.name for c in made[2:]) == sorted(["main", new.name])


async def test_a_tool_goes_to_the_chat_whose_turn_calls_it(tmp_path, made, monkeypatch):
    a = adapter(tmp_path)
    await a.connect()
    await made[0].on_notice("notifications/claude/channel", {"content": "x", "meta": {"kind": "new_chat", "nonce": "n1"}})
    other = made[1]
    env = {}
    import gateway.session_context as sc

    monkeypatch.setattr(sc, "get_session_env", lambda k, d="": env.get(k, d))
    handle = plugin.tool_handler("create_info")
    env.update({"HERMES_SESSION_PLATFORM": "trommi", "HERMES_SESSION_CHAT_ID": other.name})
    assert await handle({"title": "t"}) == f"create_info in hermes-einkauf-{other.name}"
    env.update({"HERMES_SESSION_PLATFORM": "telegram", "HERMES_SESSION_CHAT_ID": "12345"})
    assert await handle({"title": "t"}) == "create_info in hermes-einkauf"
    env.update({"HERMES_SESSION_PLATFORM": "trommi", "HERMES_SESSION_CHAT_ID": "c-ffffffffffff"})
    assert (await handle({"title": "t"})).startswith("Error: no Trommi chat")
    await a.disconnect()
    assert (await handle({})).startswith("Error: Trommi works through the Hermes gateway")


def test_the_plugin_registers_the_platform_and_the_board_tools(tmp_path, monkeypatch):
    spec = tmp_path / "trommi.json"
    spec.write_text('{"version": "connector/0.2.0", "instructions": "Trommi chat.", "tools": [{"name": "create_info", "description": "Put something to read.", "inputSchema": {"type": "object", "properties": {"title": {"type": "string"}}}}, {"name": "ask_permission", "description": "x", "inputSchema": {"type": "object"}}]}')
    monkeypatch.setattr(plugin, "SPEC_FILE", spec)
    seen = {"platform": None, "tools": []}

    class Ctx:
        def register_platform(self, **kw):
            seen["platform"] = kw

        def register_tool(self, **kw):
            seen["tools"].append(kw)

    plugin.register(Ctx())
    assert seen["platform"]["name"] == "trommi" and seen["platform"]["platform_hint"] == "Trommi chat."
    assert [t["name"] for t in seen["tools"]] == ["trommi_create_info"]
    assert seen["tools"][0]["schema"]["parameters"]["properties"] == {"title": {"type": "string"}}
    assert seen["tools"][0]["is_async"] is True
