#!/usr/bin/python3
import os
import sys
from lintian_brush.control import (
    ensure_minimum_version,
    update_control,
    )


# TODO(jelmer): Can we get these elsewhere rather than
# hardcoding them here?
minimum_debhelper_version = 9
default_debhelper_version = 11

if os.path.exists('debian/compat'):
    with open('debian/compat', 'r') as f:
        current = int(f.read().strip())
    if current < minimum_debhelper_version:
        new = default_debhelper_version
        with open('debian/compat', 'w') as f:
            f.write('%s\n' % new)
    else:
        # Nothing to do
        sys.exit(2)
else:
    sys.exit(2)


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
            control["Build-Depends"],
            "debhelper",
            new)


update_control(source_package_cb=bump_debhelper)
print("Bump debhelper from deprecated %s to %s." % (current, new))
print("Fixed-Lintian-Tags: package-uses-deprecated-debhelper-compat-version")
