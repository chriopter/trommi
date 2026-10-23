"""The Hermes gateway adapter of the platform "trommi": every Trommi chat is a Hermes chat (`chats.py`).

In: a connector's board event (`notifications/claude/channel`: the person's words, his answer on a card, a wakeup)
becomes a MessageEvent of that chat; the person's ask for a new chat (`meta.kind` "new_chat", from the main chat
only) starts one. Out: `send` is the agent's message in that chat (`trommi/answer`), `send_draft` its answer as it is
written (`trommi/preview`, shown growing, spec 7.5), typing is `trommi/working`, an approval is a yes/no card on the
board (the connector's `permission_request`) whose verdict resolves exactly that Hermes request."""
from __future__ import annotations

import logging
import secrets
from pathlib import Path
from typing import Any

from gateway.config import Platform
from gateway.platforms.base import BasePlatformAdapter, MessageEvent, MessageType, SendResult

from .chats import MAIN, Chats

logger = logging.getLogger(__name__)

# The running adapter of this process, for the board tools (plugin.py): they reach the chat whose turn calls them.
CURRENT: dict[str, "TrommiAdapter"] = {}


def _pending_request_id(session_key: str, command: str) -> str | None:
    """The Hermes request id of the approval this prompt is for: the one waiting request of this session with this
    command. None when it cannot be told (none, several alike, or a Hermes that keeps them otherwise): then a yes on the
    card allows nothing, and Hermes' own deadline denies it."""
    try:
        from tools import approval

        with approval._lock:  # noqa: SLF001 - read only; no public accessor in this Hermes version
            queue = list(approval._gateway_queues.get(session_key) or [])  # noqa: SLF001
        alike = [entry for entry in queue if str(entry.data.get("command") or "") == command]
        if len(alike) == 1 and alike[0].data.get("request_id"):
            return str(alike[0].data["request_id"])
    except Exception:  # noqa: BLE001 - another Hermes version: no correlation, no yes
        return None
    return None


class TrommiAdapter(BasePlatformAdapter):
    def __init__(self, config, *, chats: Chats | None = None, platform: Platform | None = None):
        super().__init__(config=config, platform=platform or Platform("trommi"))
        extra = getattr(config, "extra", {}) or {}
        self.instance = str(extra.get("instance") or "")
        if chats is None:
            from hermes_constants import get_hermes_home

            home = get_hermes_home()
            chats = Chats(
                program=str(extra.get("connector") or "trommi-connector"),
                instance=self.instance,
                name=str(extra.get("name") or "Hermes"),
                folder=str(extra.get("folder") or home),
                state_file=Path(home) / "state" / f"trommi-{self.instance or 'main'}.json",
                on_event=self._on_event,
            )
        else:
            chats.on_event = self._on_event
        self.chats = chats
        # approvals on the board: our request id -> (the Hermes session, its request id)
        self._asks: dict[str, tuple[str, str | None]] = {}

    @property
    def name(self) -> str:
        return "Trommi"

    @property
    def authorization_is_upstream(self) -> bool:
        # Only the person's own devices write what reaches a chat: the connector takes a human's command only after
        # the app core checked its signature and its sender (spec 9.0.9).
        return True

    async def connect(self, *, is_reconnect: bool = False) -> bool:
        try:
            await self.chats.start()
        except Exception as exc:  # noqa: BLE001 - reported to the gateway
            logger.exception("Trommi connector did not start")
            self._set_fatal_error("connect_failed", f"The Trommi connector did not start: {exc}", retryable=True)
            return False
        CURRENT[self.instance] = self
        self._mark_connected()
        return True

    async def disconnect(self) -> None:
        CURRENT.pop(self.instance, None)
        await self.chats.stop()
        self._mark_disconnected()

    # ---- in ----

    async def _on_event(self, chat: str, method: str, params: dict[str, Any]) -> None:
        if method == "notifications/claude/channel":
            meta = params.get("meta") if isinstance(params.get("meta"), dict) else {}
            if meta.get("kind") == "new_chat":
                if chat == MAIN and isinstance(meta.get("nonce"), str):
                    made = await self.chats.new_chat(meta["nonce"])
                    logger.info("Trommi: new chat %s", made)
                return
            text = str(params.get("content") or "").strip()
            if not text:
                return
            source = self.build_source(chat_id=chat, chat_name=f"Trommi {chat}", chat_type="dm", user_id="trommi", user_name="You")
            await self.handle_message(MessageEvent(
                text=text,
                message_type=MessageType.TEXT,
                user_id="trommi",
                user_name="You",
                source=source,
                message_id=f"trommi-{secrets.token_hex(8)}",
                metadata={"trommi": meta},
            ))
        elif method == "notifications/claude/channel/permission":
            ask = self._asks.pop(str(params.get("request_id") or ""), None)
            if ask is None:
                return
            session_key, hermes_request = ask
            if hermes_request is None:
                # (which request it was cannot be told: a yes allows nothing; Hermes denies it when its time runs out)
                logger.warning("Trommi: an approval could not be matched to its Hermes request; nothing allowed")
                return
            choice = "once" if params.get("behavior") == "allow" else "deny"
            from tools.approval import resolve_gateway_approval

            resolve_gateway_approval(session_key, choice, request_id=hermes_request)

    # ---- out ----

    async def _notify(self, chat_id: str, method: str, params: dict[str, Any]) -> SendResult:
        child = self.chats.child_for(chat_id)
        if child is None:
            return SendResult(success=False, error=f"no Trommi chat {chat_id}")
        try:
            await child.notify(method, params)
        except Exception as exc:  # noqa: BLE001 - the gateway retries or reports
            return SendResult(success=False, error=str(exc), retryable=True)
        return SendResult(success=True, message_id=f"trommi-{secrets.token_hex(8)}")

    async def send(self, chat_id: str, content: str, reply_to: str | None = None, metadata: dict[str, Any] | None = None) -> SendResult:
        # (a request: only once the connector wrote it to the board is it sent; answers keep their order there)
        child = self.chats.child_for(chat_id)
        if child is None:
            return SendResult(success=False, error=f"no Trommi chat {chat_id}")
        try:
            await child.request("trommi/answer", {"text": content})
        except Exception as exc:  # noqa: BLE001 - the gateway retries or reports
            return SendResult(success=False, error=str(exc), retryable=True)
        return SendResult(success=True, message_id=f"trommi-{secrets.token_hex(8)}")

    def supports_draft_streaming(self, chat_type=None, metadata=None, chat_id=None) -> bool:
        return True

    async def send_draft(self, chat_id: str, draft_id: int, content: str, metadata: dict[str, Any] | None = None) -> SendResult:
        result = await self._notify(chat_id, "trommi/preview", {"draft": str(draft_id), "text": content})
        return SendResult(success=result.success, error=result.error)

    async def send_typing(self, chat_id: str, metadata=None) -> None:
        await self._notify(chat_id, "trommi/working", {"on": True})

    async def stop_typing(self, chat_id: str) -> None:
        await self._notify(chat_id, "trommi/working", {"on": False})

    async def _send_exec_approval_prompt(self, prompt) -> SendResult:
        rid = secrets.token_hex(8)
        self._asks[rid] = (prompt.session_key, _pending_request_id(prompt.session_key, str(prompt.command or "")))
        result = await self._notify(prompt.chat_id, "notifications/claude/channel/permission_request", {
            "request_id": rid,
            "tool_name": "Hermes",
            "description": str(prompt.description or "Hermes asks to run a command"),
            "input_preview": str(prompt.command or "")[:2000],
        })
        if not result.success:
            self._asks.pop(rid, None)
        return result

    async def get_chat_info(self, chat_id: str) -> dict[str, Any]:
        return {"chat_id": chat_id, "name": f"Trommi {chat_id}", "type": "dm"}

    # ---- the board tools (plugin.py) ----

    async def call_tool(self, chat: str, name: str, arguments: dict[str, Any]) -> str:
        child = self.chats.child_for(chat)
        if child is None:
            return f"Error: no Trommi chat {chat}"
        return await child.call_tool(name, arguments)
