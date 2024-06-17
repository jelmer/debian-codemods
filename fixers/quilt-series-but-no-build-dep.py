#!/usr/bin/python3

import os
import sys
from typing import Optional

from debmutate.control import ensure_some_version

from lintian_brush.fixer import (
    control,
    fixed_lintian_tag,
    is_debcargo_package,
    report_result,
)

if is_debcargo_package():
    sys.exit(0)

format: Optional[str]
try:
    with open("debian/source/format") as f:
        format = f.read().strip()
except FileNotFoundError:
    format = None

if format != "3.0 (quilt)" and os.path.exists("debian/patches/series"):
    with control as updater:
        updater.source["Build-Depends"] = ensure_some_version(
            updater.source.get("Build-Depends", ""), "quilt"
        )
    if updater.changed:
        fixed_lintian_tag(updater.source, "quilt-series-but-no-build-dep")


report_result("Add missing dependency on quilt.")
