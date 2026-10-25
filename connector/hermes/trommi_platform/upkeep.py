"""Keeping the add-on current, and Hermes' other platforms as they were when Trommi turned live text on.

A newer Trommi connector writes this add-on again when it starts (its folder in the Hermes home); the gateway keeps
running the add-on it loaded until it starts again. `watch_addon` notices that the add-on on disk changed and, at a
moment when no turn runs, has the gateway restart itself as Hermes' own /restart does: no new turn starts, a turn that
began meanwhile ends, then the gateway starts again (under a service manager through it). Only the add-on's own
files count, not its `.release` stamp nor its README: a connector release that leaves the add-on as it was restarts
nothing.

`setup hermes` turns Hermes' streaming on for the answer that grows on the board (`streaming.enabled` is one switch
for every platform). When it was off before, setup says so in `platforms.trommi.extra.quiet_others`, and
`keep_others_quiet` has every other platform of that Hermes resolve to "no streaming" where it would follow the switch:
a platform's own setting (`display.platforms.<name>.streaming`) still wins."""
from __future__ import annotations

import asyncio
import hashlib
import logging
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

# The add-on's folder (trommi-platform/ in the Hermes home): this package and trommi.json beside it.
PACKAGE = Path(__file__).resolve().parent.parent
# Files that change with every release whether the add-on did or not, and its words for people.
STAMPS = {".release", "README.md"}


def fingerprint(root: Path = PACKAGE) -> str | None:
    """The add-on's files as one hash; None while it cannot be read whole (a write that moves it in place)."""
    digest = hashlib.sha256()
    try:
        files = sorted(p for p in root.rglob("*") if p.is_file() and "__pycache__" not in p.parts and p.name not in STAMPS and not p.name.endswith(".pyc"))
        if not files:
            return None
        for path in files:
            digest.update(str(path.relative_to(root)).encode())
            digest.update(b"\0")
            digest.update(path.read_bytes())
            digest.update(b"\0")
    except OSError:
        return None
    return digest.hexdigest()


# What this process loaded.
LOADED = fingerprint()


def busy(runner: Any) -> bool:
    """Whether the gateway runs a turn now (any platform's); unknown counts as not."""
    count = getattr(runner, "_running_agent_count", None)
    try:
        return bool(count()) if callable(count) else False
    except Exception:  # noqa: BLE001 - another Hermes version
        return False


def restart_gateway(runner: Any) -> bool:
    """Has the gateway restart itself as /restart does; whether it was asked (or restarts already)."""
    if runner is None:
        return False
    if getattr(runner, "_restart_requested", False) or getattr(runner, "_draining", False):
        return True
    request = getattr(runner, "request_restart", None)
    if not callable(request):
        return False
    try:
        from gateway.restart import is_container_restart_context, is_gateway_supervisor_process

        via_service = bool(is_gateway_supervisor_process() or is_container_restart_context())
    except Exception:  # noqa: BLE001 - another Hermes version: started again on its own
        via_service = False
    try:
        request(detached=not via_service, via_service=via_service)
    except Exception:  # noqa: BLE001
        logger.exception("Trommi: the gateway was not restarted")
        return False
    return True


async def watch_addon(adapter: Any, every: float, loaded: str | None = None, root: Path = PACKAGE) -> bool:
    """Looks at the add-on every `every` seconds; once it changed, restarts the gateway when no turn runs. Returns
    whether a restart was asked (the gateway then ends this task), False when it cannot be (Hermes loads the new
    add-on at its next start)."""
    loaded = LOADED if loaded is None else loaded
    if loaded is None:
        return False
    while True:
        await asyncio.sleep(every)
        now = fingerprint(root)
        if now is None or now == loaded:
            continue
        runner = getattr(adapter, "gateway_runner", None)
        if busy(runner):
            continue
        logger.info("Trommi: a newer add-on is in place; the gateway restarts to load it")
        if restart_gateway(runner):
            return True
        logger.warning("Trommi: a newer add-on is in place; Hermes loads it at its next start")
        return False


def _quiet(user_config: Any) -> bool:
    """Whether this Hermes' setup turned streaming on for Trommi (`platforms.trommi.extra.quiet_others`)."""
    try:
        value = user_config["platforms"]["trommi"]["extra"]["quiet_others"]
    except (KeyError, TypeError):
        return False
    return value is True or str(value).lower() in {"true", "1", "yes", "on"}


def keep_others_quiet() -> bool:
    """Hermes' display settings, as each turn resolves them: for a Hermes whose setup turned streaming on for Trommi,
    every other platform that would follow that switch keeps its "no streaming". Read from the config of the turn's
    own Hermes (one profile does not change another's) and at each turn (a platform registered later counts too); a
    platform's own setting, or its tier's, still wins. Whether it is in place."""
    try:
        from gateway import display_config
    except ImportError:
        return False
    original = getattr(display_config, "resolve_display_setting", None)
    if not callable(original):
        return False
    if getattr(original, "_trommi_quiet", False):
        return True

    def resolve(user_config: dict, platform_key: str, setting: str, fallback: Any = None) -> Any:
        value = original(user_config, platform_key, setting, fallback)
        if setting == "streaming" and value is None and platform_key != "trommi" and _quiet(user_config):
            return False
        return value

    resolve._trommi_quiet = True  # type: ignore[attr-defined]
    resolve.__wrapped__ = original  # type: ignore[attr-defined]
    display_config.resolve_display_setting = resolve
    return True
