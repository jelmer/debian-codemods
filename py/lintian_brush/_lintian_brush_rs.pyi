# Stub file for lintian_brush._lintian_brush_rs

from typing import Any

DEBHELPER_VERSIONS: dict[str, Any]
DPKG_VERSIONS: dict[str, Any]

# Based on usage in vcs.py
def determine_gitlab_browser_url(url: str) -> str | None: ...
def determine_browser_url(url: str) -> str | None: ...
def canonicalize_vcs_browser_url(url: str) -> str: ...

# Based on usage in __init__.py
def get_builtin_fixer_lintian_tags() -> list[str]: ...
