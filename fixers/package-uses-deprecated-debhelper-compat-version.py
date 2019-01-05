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
default_debhelper_version = 12

compat_release = os.environ.get('COMPAT_RELEASE', 'sid')
if compat_release == 'sid':
    new_debhelper_version = 12
elif compat_release == 'stretch':
    new_debhelper_version = 10
else:
    new_debhelper_version = minimum_debhelper_version

if os.path.exists('debian/compat'):
    with open('debian/compat', 'r') as f:
        current_debhelper_version = int(f.read().strip())
    if current_debhelper_version < new_debhelper_version:
        with open('debian/compat', 'w') as f:
            f.write('%s\n' % new_debhelper_version)
    else:
        # Nothing to do
        sys.exit(2)
else:
    sys.exit(2)


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
            control["Build-Depends"],
            "debhelper",
            "%d~" % new_debhelper_version)


update_control(source_package_cb=bump_debhelper)
if current_debhelper_version < minimum_debhelper_version:
    kind = "deprecated"
    tag = "package-uses-deprecated-debhelper-compat-version"
else:
    kind = "old"
    tag = "package-uses-old-debhelper-compat-version"
print("Bump debhelper from %s %s to %s." % (
    kind, current_debhelper_version, new_debhelper_version))
print("Fixed-Lintian-Tags: %s" % tag)
