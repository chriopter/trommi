"""One Trommi chat's connector: the `trommi-connector` program as a child process, spoken to over its stdin and
stdout with the MCP's JSON-RPC lines (one JSON object per line). It holds that chat's device key; this process holds
none. What the connector says unasked (the board's events, a verdict on an approval) goes to `on_notice`.

Writes are serialized; every request has its own id and a deadline; stdout and stderr are read by tasks of their own;
the child is started again with a growing pause when it ends by itself, and never written to blindly after a restart
(a request that was on its way when it ended fails, it is not sent again)."""
from __future__ import annotations

import asyncio
import json
import logging
import os
from collections.abc import Awaitable, Callable
from typing import Any

logger = logging.getLogger(__name__)

PROTOCOL = "2025-06-18"
REQUEST_SECONDS = 120.0
RESTART_MIN, RESTART_MAX = 1.0, 60.0
LINE_MAX = 16 * 1024 * 1024

Notice = Callable[[str, dict[str, Any]], Awaitable[None]]


class ChildError(RuntimeError):
    pass


class Child:
    def __init__(self, program: str, env: dict[str, str], on_notice: Notice, *, name: str = "trommi"):
        self.program = program
        self.env = env
        self.on_notice = on_notice
        self.name = name
        self._proc: asyncio.subprocess.Process | None = None
        self._next = 0
        self._waiting: dict[int, asyncio.Future] = {}
        self._write = asyncio.Lock()
        self._tasks: list[asyncio.Task] = []
        self._stopping = False
        self._ready = asyncio.Event()
        self._supervisor: asyncio.Task | None = None

    async def start(self) -> None:
        self._stopping = False
        self._supervisor = asyncio.create_task(self._supervise())
        await asyncio.wait_for(self._ready.wait(), timeout=REQUEST_SECONDS)

    async def _supervise(self) -> None:
        pause = RESTART_MIN
        while not self._stopping:
            try:
                await self._spawn()
                pause = RESTART_MIN
                assert self._proc is not None
                await self._proc.wait()
            except asyncio.CancelledError:
                raise
            except Exception:  # noqa: BLE001 - a child that does not start is tried again
                logger.exception("Trommi connector %s did not start", self.name)
            self._ready.clear()
            self._fail_waiting("the Trommi connector ended")
            if self._stopping:
                return
            logger.warning("Trommi connector %s ended; starting it again in %.0f s", self.name, pause)
            await asyncio.sleep(pause)
            pause = min(RESTART_MAX, pause * 2)

    async def _spawn(self) -> None:
        env = {**os.environ, **self.env}
        self._proc = await asyncio.create_subprocess_exec(
            self.program,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
            limit=LINE_MAX,
        )
        for t in self._tasks:
            t.cancel()
        self._tasks = [asyncio.create_task(self._read_out()), asyncio.create_task(self._read_err())]
        await self._request("initialize", {
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": {"name": "hermes-trommi", "version": "1"},
        })
        await self.notify("notifications/initialized", {})
        self._ready.set()

    async def _read_out(self) -> None:
        assert self._proc is not None and self._proc.stdout is not None
        while True:
            line = await self._proc.stdout.readline()
            if not line:
                return
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if not isinstance(msg, dict):
                continue
            if "id" in msg and ("result" in msg or "error" in msg):
                fut = self._waiting.pop(msg.get("id"), None)
                if fut and not fut.done():
                    if "error" in msg:
                        fut.set_exception(ChildError(str((msg.get("error") or {}).get("message", "error"))))
                    else:
                        fut.set_result(msg.get("result"))
                continue
            method = msg.get("method")
            if isinstance(method, str):
                params = msg.get("params") if isinstance(msg.get("params"), dict) else {}
                try:
                    await self.on_notice(method, params)
                except Exception:  # noqa: BLE001 - one event never stops the reader
                    logger.exception("Trommi event not handled")

    async def _read_err(self) -> None:
        assert self._proc is not None and self._proc.stderr is not None
        while True:
            line = await self._proc.stderr.readline()
            if not line:
                return
            # (the connector's own log lines; board content is never logged by it)
            logger.debug("trommi[%s]: %s", self.name, line.decode(errors="replace").rstrip())

    def _fail_waiting(self, why: str) -> None:
        for fut in self._waiting.values():
            if not fut.done():
                fut.set_exception(ChildError(why))
        self._waiting.clear()

    async def _send(self, msg: dict[str, Any]) -> None:
        proc = self._proc
        if proc is None or proc.stdin is None or proc.returncode is not None:
            raise ChildError("the Trommi connector is not running")
        async with self._write:
            proc.stdin.write((json.dumps(msg, ensure_ascii=False) + "\n").encode())
            await proc.stdin.drain()

    async def _request(self, method: str, params: dict[str, Any], timeout: float = REQUEST_SECONDS) -> Any:
        self._next += 1
        rid = self._next
        fut: asyncio.Future = asyncio.get_running_loop().create_future()
        self._waiting[rid] = fut
        try:
            await self._send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
            return await asyncio.wait_for(fut, timeout=timeout)
        finally:
            self._waiting.pop(rid, None)

    async def request(self, method: str, params: dict[str, Any], timeout: float = REQUEST_SECONDS) -> Any:
        await asyncio.wait_for(self._ready.wait(), timeout=timeout)
        return await self._request(method, params, timeout)

    async def notify(self, method: str, params: dict[str, Any]) -> None:
        await self._send({"jsonrpc": "2.0", "method": method, "params": params})

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> str:
        """A board tool of this chat: its text, or the tool's own error as text."""
        result = await self.request("tools/call", {"name": name, "arguments": arguments})
        parts = (result or {}).get("content") or []
        text = "\n".join(p.get("text", "") for p in parts if isinstance(p, dict))
        if (result or {}).get("isError"):
            return f"Error: {text}"
        return text

    async def stop(self) -> None:
        self._stopping = True
        proc = self._proc
        if proc is not None and proc.returncode is None:
            try:
                if proc.stdin is not None:
                    proc.stdin.close()
                await asyncio.wait_for(proc.wait(), timeout=5)
            except (asyncio.TimeoutError, ProcessLookupError):
                proc.terminate()
                try:
                    await asyncio.wait_for(proc.wait(), timeout=5)
                except asyncio.TimeoutError:
                    proc.kill()
        if self._supervisor is not None:
            self._supervisor.cancel()
        for t in self._tasks:
            t.cancel()
        self._fail_waiting("stopped")
