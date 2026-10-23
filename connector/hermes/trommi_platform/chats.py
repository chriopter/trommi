"""The Trommi chats of one Hermes: one connector child each (`child.py`), under a chat id that is also the Hermes
chat id on the platform "trommi". "main" is the chat this Hermes connected with; one that the person asks for with
"New chat with …" gets "c-<random>". Which chats there are is kept in a file of the Hermes home, readable by this
user only; the device keys stay with the connector, beside its other keys."""
from __future__ import annotations

import asyncio
import json
import logging
import os
import re
import secrets
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

from .child import Child

logger = logging.getLogger(__name__)

MAIN = "main"
CHAT_ID = re.compile(r"^(main|c-[0-9a-f]{12})$")
CHATS_MAX = 8
NONCES_KEPT = 64

Event = Callable[[str, str, dict[str, Any]], Awaitable[None]]


def slug_of(name: str) -> str:
    """A name's letters and digits, lower case, at most 32 ("Einkauf" → "einkauf")."""
    return "".join(c.lower() for c in name if c.isascii() and c.isalnum())[:32]


class Chats:
    def __init__(
        self,
        *,
        program: str,
        instance: str,
        name: str,
        folder: str,
        state_file: Path,
        on_event: Event,
        make_child: Callable[..., Child] = Child,
    ):
        self.program = program
        self.slug = slug_of(instance)
        self.name = name or "Hermes"
        self.folder = folder
        self.state_file = state_file
        self.on_event = on_event
        self.make_child = make_child
        self.children: dict[str, Child] = {}
        self._nonces: list[str] = []
        self._lock = asyncio.Lock()

    # ---- the kept list ----

    def _read(self) -> dict[str, Any]:
        try:
            data = json.loads(self.state_file.read_text())
            return data if isinstance(data, dict) else {}
        except (OSError, ValueError):
            return {}

    def _write(self) -> None:
        self.state_file.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.state_file.with_name(f".{self.state_file.name}.{os.getpid()}")
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as f:
            json.dump({"chats": sorted(self.children), "nonces": self._nonces[-NONCES_KEPT:]}, f)
        os.replace(tmp, self.state_file)

    # ---- the children ----

    def session_key(self, chat: str) -> str:
        base = f"hermes-{self.slug}" if self.slug else "hermes"
        return base if chat == MAIN else f"{base}-{chat}"

    def env(self, chat: str, name: str | None = None) -> dict[str, str]:
        """A chat's connector's environment; `name` the name a new chat proposes ("Hermes Einkauf 2"): the board keeps
        the name it was activated with."""
        return {
            "TROMMI_HOST": "hermes",
            "TROMMI_INSTANCE": self.slug,
            "TROMMI_AGENT_NAME": name or self.name,
            "TROMMI_SESSION_KEY": self.session_key(chat),
            "TROMMI_FOLDER": self.folder,
            "TROMMI_CHANNEL_EVENTS": "on",
        }

    def _child(self, chat: str, name: str | None = None) -> Child:
        async def notice(method: str, params: dict[str, Any]) -> None:
            await self.on_event(chat, method, params)

        return self.make_child(self.program, self.env(chat, name), notice, name=chat)

    async def start(self) -> None:
        kept = self._read()
        self._nonces = [n for n in kept.get("nonces", []) if isinstance(n, str)][-NONCES_KEPT:]
        chats = [c for c in kept.get("chats", []) if isinstance(c, str) and CHAT_ID.match(c)]
        for chat in [MAIN, *[c for c in chats if c != MAIN]][:CHATS_MAX]:
            child = self._child(chat)
            self.children[chat] = child
            await child.start()
        self._write()

    async def stop(self) -> None:
        for child in list(self.children.values()):
            await child.stop()
        self.children.clear()

    def child_for(self, chat: str | None) -> Child | None:
        """The child of a Trommi chat; None for an id that is not one (fail closed)."""
        return self.children.get(chat or MAIN)

    async def new_chat(self, nonce: str) -> str | None:
        """Starts a new chat for the person's ask (`nonce`, once each): its child, which activates itself on the
        approved computer (its `connect` tool, no link). Returns its chat id; None for a nonce seen before, or past the
        limit."""
        async with self._lock:
            if nonce in self._nonces:
                return None
            self._nonces.append(nonce)
            if len(self.children) >= CHATS_MAX:
                self._write()
                return None
            chat = f"c-{secrets.token_hex(6)}"
            child = self._child(chat, f"{self.name} {len(self.children) + 1}")
            self.children[chat] = child
            self._write()
        await child.start()
        said = await child.call_tool("connect", {})
        logger.info("Trommi chat %s: %s", chat, said[:200])
        return chat
