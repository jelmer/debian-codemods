#!/usr/bin/python3

import os
import sys

from lintian_brush.control import (
    add_dependency,
    get_debhelper_compat_version,
    update_control,
    )


compat_version = get_debhelper_compat_version()
if compat_version is None or compat_version <= 11:
    # N/A
    sys.exit(0)


added = set()


def check_pre_depends(control):
    name = control["Package"]
    if not os.path.exists('debian/%s.init' % name):
        return
    if not (os.path.exists('debian/%s.service' % name) or
            os.path.exists('debian/%s.upstart' % name)):
        return
    if "${misc:Pre-Depends}" in control.get("Pre-Depends", ""):
        return
    control["Pre-Depends"] = add_dependency(
        control.get("Pre-Depends", ""),
        "${misc:Pre-Depends}")
    added.add(name)


update_control(binary_package_cb=check_pre_depends)
# Add Pre-Depends: ${misc:Pre-Depends} iff:
# - a package has both a init script and a (ubuntu | systemd) unit

print("Add missing Pre-Depends: ${misc:Pre-Depends} in %s." %
      ", ".join(sorted(added)))
print("Fixed-Lintian-Tags: skip-systemd-native-flag-missing-pre-depends")
