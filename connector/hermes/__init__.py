"""Hermes directory-plugin entry point of the Trommi platform (written by `trommi-connector setup hermes`)."""

try:  # Hermes' filesystem plugin namespace
    from .trommi_platform.plugin import register
except ImportError:  # a source checkout under pytest
    from trommi_platform.plugin import register

__all__ = ["register"]
