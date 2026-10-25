"""upkeep.py: a newer add-on restarts the gateway when no turn runs; live text for Trommi leaves the others quiet."""
from __future__ import annotations

import asyncio
from types import SimpleNamespace

from trommi_platform import upkeep


def addon(tmp_path):
    (tmp_path / "trommi_platform").mkdir()
    (tmp_path / "trommi_platform" / "adapter.py").write_text("one")
    (tmp_path / "trommi.json").write_text("{}")
    (tmp_path / ".release").write_text("7")
    (tmp_path / "README.md").write_text("words")
    return tmp_path


class Runner:
    def __init__(self, running=0):
        self.running = running
        self.asked = []

    def _running_agent_count(self):
        return self.running

    def request_restart(self, *, detached=False, via_service=False):
        self.asked.append((detached, via_service))
        return True


def test_only_the_addons_own_files_count(tmp_path):
    root = addon(tmp_path)
    before = upkeep.fingerprint(root)
    (root / ".release").write_text("8")
    (root / "README.md").write_text("other words")
    assert upkeep.fingerprint(root) == before
    (root / "trommi_platform" / "adapter.py").write_text("two")
    assert upkeep.fingerprint(root) != before


async def test_a_newer_addon_restarts_the_gateway_once_no_turn_runs(tmp_path, monkeypatch):
    monkeypatch.setattr("gateway.restart.is_gateway_supervisor_process", lambda *a, **k: True)
    root = addon(tmp_path)
    loaded = upkeep.fingerprint(root)
    runner = Runner(running=1)
    adapter = SimpleNamespace(gateway_runner=runner)
    watch = asyncio.create_task(upkeep.watch_addon(adapter, 0.01, loaded, root))
    await asyncio.sleep(0.05)
    assert not watch.done() and runner.asked == [], "the same add-on: nothing"
    (root / "trommi_platform" / "adapter.py").write_text("two")
    await asyncio.sleep(0.05)
    assert not watch.done() and runner.asked == [], "a turn runs: it waits"
    runner.running = 0
    assert await asyncio.wait_for(watch, 1) is True
    assert runner.asked == [(False, True)], "as /restart under a service manager"


async def test_without_a_runner_it_waits_for_the_next_start(tmp_path):
    root = addon(tmp_path)
    loaded = upkeep.fingerprint(root)
    (root / "trommi_platform" / "adapter.py").write_text("two")
    assert await asyncio.wait_for(upkeep.watch_addon(SimpleNamespace(), 0.01, loaded, root), 1) is False


def test_the_other_platforms_stay_without_streaming_when_setup_turned_it_on(monkeypatch):
    from gateway import display_config

    monkeypatch.setattr(display_config, "resolve_display_setting", display_config.resolve_display_setting)
    assert upkeep.keep_others_quiet() and upkeep.keep_others_quiet(), "in place once"
    resolve = display_config.resolve_display_setting
    assert getattr(resolve.__wrapped__, "_trommi_quiet", False) is False, "not wrapped twice"
    quiet = {"platforms": {"trommi": {"extra": {"quiet_others": True}}}}
    # (this Hermes: setup turned streaming on; another profile's config: it did not)
    assert resolve(quiet, "telegram", "streaming") is False
    assert resolve({}, "telegram", "streaming") is None
    assert resolve(quiet, "trommi", "streaming") is not False
    # a platform's own setting wins, and so does a tier's "no"
    own = {**quiet, "display": {"platforms": {"telegram": {"streaming": True}}}}
    assert resolve(own, "telegram", "streaming") is True
    assert resolve(quiet, "webhook", "streaming") is False
    # a platform registered later counts too, its other defaults untouched
    from gateway.platform_registry import PlatformEntry, platform_registry

    if not platform_registry.is_registered("latecomer"):
        platform_registry.register(PlatformEntry(name="latecomer", label="Late", adapter_factory=lambda _cfg: None, check_fn=lambda: True, display_tier="high"))
    assert resolve(quiet, "latecomer", "streaming") is False
    assert resolve(quiet, "latecomer", "tool_preview_length") == display_config.resolve_display_setting.__wrapped__({}, "latecomer", "tool_preview_length")
    # other settings pass through
    assert resolve(quiet, "telegram", "tool_progress") == resolve({}, "telegram", "tool_progress")
