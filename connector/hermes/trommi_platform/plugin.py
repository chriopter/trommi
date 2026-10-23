"""Hermes plugin registration of the Trommi platform: the adapter (adapter.py) and the board tools.

The board tools are the connector's own (cards, status lines, knowledge, wakeups, …), named `trommi_<name>`, with the
schemas and the instructions the connector wrote next to this add-on (`trommi.json`, by `trommi-connector setup
hermes`): registering them starts nothing. A call goes to the Trommi chat whose turn makes it (the session's platform
"trommi" and its chat id); from any other conversation of this Hermes (Telegram, cron) to the main chat."""
from __future__ import annotations

import json
import logging
import shutil
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

SPEC_FILE = Path(__file__).resolve().parent.parent / "trommi.json"
# (Claude Code's woken runs only: Hermes asks through its own approvals)
LEFT_OUT = {"ask_permission"}

TROMMI_DISPLAY_DEFAULTS: dict[str, Any] = {
    "tool_progress": "off",
    "thinking_progress": False,
    "interim_assistant_messages": False,
    "long_running_notifications": False,
    "busy_ack_detail": False,
    # (the answer grows on the board while it is written, where Hermes streams at all: `streaming.enabled`)
    "streaming": True,
}


def read_spec() -> dict[str, Any]:
    try:
        spec = json.loads(SPEC_FILE.read_text())
        return spec if isinstance(spec, dict) else {}
    except (OSError, ValueError):
        return {}


def register_display_defaults() -> None:
    """The board shows what Hermes does by itself (working, live text): no chatter of the gateway in the chat."""
    try:
        from gateway import display_config
    except ImportError:
        return
    platform_defaults = getattr(display_config, "_PLATFORM_DEFAULTS", None)
    if not isinstance(platform_defaults, dict):
        return
    defaults = platform_defaults.setdefault("trommi", {})
    if isinstance(defaults, dict):
        for key, value in TROMMI_DISPLAY_DEFAULTS.items():
            defaults.setdefault(key, value)


def _program(config) -> str:
    extra = getattr(config, "extra", {}) or {}
    return str(extra.get("connector") or "trommi-connector")


def check_requirements() -> bool:
    return SPEC_FILE.is_file()


def validate_config(config) -> bool:
    program = _program(config)
    return check_requirements() and (Path(program).is_file() or shutil.which(program) is not None)


def is_connected(config) -> bool:
    return bool(getattr(config, "enabled", False)) and validate_config(config)


def interactive_setup() -> None:
    print("Set Trommi up with the connector: trommi-connector setup hermes [--name <name>] [--profile <profile>]")


def chat_of_call() -> str:
    """The Trommi chat a tool call is for: this turn's chat on the platform "trommi", else the main chat."""
    from gateway.session_context import get_session_env

    if get_session_env("HERMES_SESSION_PLATFORM", "") == "trommi":
        return get_session_env("HERMES_SESSION_CHAT_ID", "") or "main"
    return "main"


def tool_handler(name: str):
    async def handle(args: dict[str, Any], **_kw) -> str:
        from .adapter import CURRENT

        adapter = next(iter(CURRENT.values()), None)
        if adapter is None:
            return "Error: Trommi works through the Hermes gateway; it is not running here (hermes gateway)."
        return await adapter.call_tool(chat_of_call(), name, args if isinstance(args, dict) else {})

    return handle


def register_tools(ctx, spec: dict[str, Any]) -> int:
    count = 0
    for tool in spec.get("tools") or []:
        name = tool.get("name") if isinstance(tool, dict) else None
        if not isinstance(name, str) or name in LEFT_OUT:
            continue
        description = str(tool.get("description") or "")
        parameters = tool.get("inputSchema") if isinstance(tool.get("inputSchema"), dict) else {"type": "object", "properties": {}}
        full = f"trommi_{name}"
        ctx.register_tool(
            name=full,
            toolset="trommi",
            schema={"name": full, "description": description, "parameters": parameters},
            handler=tool_handler(name),
            is_async=True,
            description=description,
            emoji="🛎",
        )
        count += 1
    return count


def register(ctx) -> None:
    from .adapter import TrommiAdapter

    register_display_defaults()
    spec = read_spec()
    ctx.register_platform(
        name="trommi",
        label="Trommi",
        adapter_factory=lambda cfg: TrommiAdapter(cfg),
        check_fn=check_requirements,
        validate_config=validate_config,
        is_connected=is_connected,
        install_hint="Set it up with the Trommi connector: trommi-connector setup hermes",
        setup_fn=interactive_setup,
        max_message_length=60_000,
        emoji="🛎",
        pii_safe=False,
        allow_update_command=False,
        platform_hint=str(spec.get("instructions") or "You are in a Trommi chat: the person writes to you on the board."),
    )
    register_tools(ctx, spec)
