#!/usr/bin/python3

import sys

from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    read_debhelper_compat_file,
)

from lintian_brush.fixer import (
    control,
    fixed_lintian_tag,
    report_result,
)

# Debian source package is not obliged to contain `debian/compat'.
# Firstly, it may not use debhelper; secondly it may use modern
# `debhelper-compat' dependency style.


try:
    minimum_version = read_debhelper_compat_file("debian/compat")
except FileNotFoundError:
    sys.exit(0)

with control as updater:
    if ensure_minimum_debhelper_version(
        updater.source, f"{minimum_version}~"
    ):
        fixed_lintian_tag(
            "source",
            "no-versioned-debhelper-prerequisite",
            info=f"{minimum_version}"
        )


report_result(
    f"Bump debhelper dependency to >= {minimum_version}, since that's what is "
    "used in debian/compat."
)
