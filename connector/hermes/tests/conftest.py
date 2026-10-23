"""The add-on's tests run against a Hermes Agent checkout: HERMES_AGENT_SRC names it (the pinned version, README)."""
from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest

root = Path(os.getenv("HERMES_AGENT_SRC", "/usr/local/lib/hermes-agent"))
if root.exists():
    sys.path.insert(0, str(root))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


@pytest.fixture(autouse=True)
def isolate_gateway_runtime_status(monkeypatch):
    """Unit tests never write the live Hermes gateway status file."""
    from gateway.platforms.base import BasePlatformAdapter

    monkeypatch.setattr(BasePlatformAdapter, "_write_runtime_status_safe", lambda *_args, **_kwargs: None)


@pytest.fixture(autouse=True, scope="session")
def trommi_platform_registered():
    """As the plugin's registration does in Hermes: the platform "trommi" exists."""
    from gateway.platform_registry import PlatformEntry, platform_registry

    if not platform_registry.is_registered("trommi"):
        platform_registry.register(PlatformEntry(name="trommi", label="Trommi", adapter_factory=lambda _cfg: None, check_fn=lambda: True))
