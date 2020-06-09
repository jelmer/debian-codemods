#!/usr/bin/python3

import os
import sys

from debmutate.control import (
    add_dependency,
    ControlEditor,
    )
from debmutate.debhelper import (
    get_debhelper_compat_level,
    )
from lintian_brush.fixer import report_result


compat_version = get_debhelper_compat_level()
if compat_version is None or compat_version <= 11:
    # N/A
    sys.exit(0)


added = set()


# Add Pre-Depends: ${misc:Pre-Depends} iff:
# - a package has both a init script and a (ubuntu | systemd) unit

with ControlEditor() as updater:
    for binary in updater.binaries:
        name = binary["Package"]
        if not os.path.exists('debian/%s.init' % name):
            continue
        if not (os.path.exists('debian/%s.service' % name) or
                os.path.exists('debian/%s.upstart' % name)):
            continue
        if "${misc:Pre-Depends}" in binary.get("Pre-Depends", ""):
            continue
        binary["Pre-Depends"] = add_dependency(
            binary.get("Pre-Depends", ""),
            "${misc:Pre-Depends}")
        added.add(name)


report_result(
    "Add missing Pre-Depends: ${misc:Pre-Depends} in %s." %
    ", ".join(sorted(added)),
    fixed_lintian_tags=['skip-systemd-native-flag-missing-pre-depends'])
