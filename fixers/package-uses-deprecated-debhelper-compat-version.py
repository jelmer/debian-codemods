#!/usr/bin/python3
import os
import sys
from debian.changelog import Version
from lintian_brush.control import (
    ensure_exact_version,
    ensure_minimum_version,
    get_relation,
    update_control,
    )

# TODO(jelmer): Can we get these elsewhere rather than
# hardcoding them here?
minimum_debhelper_version = 9

compat_release = os.environ.get('COMPAT_RELEASE', 'sid')
if compat_release == 'sid':
    new_debhelper_compat_version = 12
elif compat_release == 'stretch':
    new_debhelper_compat_version = 10
else:
    new_debhelper_compat_version = minimum_debhelper_version

if os.path.exists('debian/compat'):
    with open('debian/compat', 'r') as f:
        current_debhelper_compat_version = int(f.read().strip())
    if current_debhelper_compat_version < new_debhelper_compat_version:
        with open('debian/compat', 'w') as f:
            f.write('%s\n' % new_debhelper_compat_version)
    else:
        # Nothing to do
        sys.exit(2)

    def bump_debhelper(control):
        control["Build-Depends"] = ensure_minimum_version(
                control["Build-Depends"],
                "debhelper",
                "%d~" % new_debhelper_compat_version)

    update_control(source_package_cb=bump_debhelper)
else:
    # Assume that the compat version is set in Build-Depends
    def bump_debhelper_compat(control):
        global current_debhelper_compat_version
        try:
            debhelper_compat_relation = get_relation(
                control["Build-Depends"], "debhelper-compat")
        except KeyError:
            sys.exit(2)
        else:
            if len(debhelper_compat_relation) > 1:
                # Not sure how to deal with this..
                sys.exit(2)
            if debhelper_compat_relation[0].version[0] != '=':
                # Not sure how to deal with this..
                sys.exit(2)
            current_debhelper_compat_version = Version(
                debhelper_compat_relation[0].version[1])
        if current_debhelper_compat_version < new_debhelper_compat_version:
            control["Build-Depends"] = ensure_exact_version(
                    control["Build-Depends"],
                    "debhelper-compat",
                    "%d" % new_debhelper_compat_version)

    update_control(source_package_cb=bump_debhelper_compat)


if current_debhelper_compat_version < minimum_debhelper_version:
    kind = "deprecated"
    tag = "package-uses-deprecated-debhelper-compat-version"
else:
    kind = "old"
    tag = "package-uses-old-debhelper-compat-version"
print("Bump debhelper from %s %s to %s." % (
    kind, current_debhelper_compat_version, new_debhelper_compat_version))
print("Fixed-Lintian-Tags: %s" % tag)
