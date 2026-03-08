"""Configuration helpers."""

from __future__ import annotations

import os
import platform
from pathlib import Path


APP_NAME = "ai-shared-memory"
DB_ENV_VAR = "AI_SHARED_MEMORY_DB_PATH"


def get_database_path() -> Path:
    """Return the configured SQLite database path."""
    configured = os.environ.get(DB_ENV_VAR)
    if configured:
        return Path(configured).expanduser().resolve()

    system = platform.system().lower()
    home = Path.home()

    if system == "darwin":
        base = home / "Library" / "Application Support"
    elif system == "windows":
        appdata = os.environ.get("APPDATA")
        base = Path(appdata) if appdata else home / "AppData" / "Roaming"
    else:
        xdg_data_home = os.environ.get("XDG_DATA_HOME")
        base = Path(xdg_data_home) if xdg_data_home else home / ".local" / "share"

    return (base / APP_NAME / "memory.db").resolve()
