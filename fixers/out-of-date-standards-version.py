#!/usr/bin/python3

import os
from lintian_brush.control import update_control, get_relation
from debian.deb822 import Deb822


# Dictionary mapping source and target versions
upgrade_path = {
    "4.1.0": "4.1.1",
    "4.2.0": "4.2.1",
    "4.3.0": "4.4.0",
}


def check_4_1_1():
    return os.path.exists("debian/changelog")


def check_4_4_0():
    """Check that the package uses debhelper."""
    if os.path.exists("debian/compat"):
        return True
    with open('debian/control') as f:
        source = next(Deb822.iter_paragraphs(f))
        build_deps = source.get('Build-Depends', '')
        try:
            get_relation(build_deps, 'debhelper-compat')
        except KeyError:
            return False
        else:
            return True


check_requirements = {
    "4.1.1": check_4_1_1,
    "4.4.0": check_4_4_0,
}


def bump_standards_version(control):
    current_version = control.get("Standards-Version")
    if current_version is None:
        return
    while current_version in upgrade_path:
        target_version = upgrade_path[current_version]
        try:
            check_fn = check_requirements[target_version]
        except KeyError:
            pass
        else:
            if not check_fn():
                break
        current_version = target_version
    control["Standards-Version"] = current_version


update_control(source_package_cb=bump_standards_version)

print('Update standards version, no changes needed.')
print('Certainty: certain')
print('Fixed-Lintian-Tags: out-of-date-standards-version')
