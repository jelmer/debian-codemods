#!/usr/bin/python3

import os
from lintian_brush.control import update_control


# Dictionary mapping source and target versions
upgrade_path = {
    "4.1.0": "4.1.1",
    "4.2.0": "4.2.1",
}


def check_4_1_1():
    return os.path.exists("debian/changelog")


check_requirements = {
    "4.1.1": check_4_1_1,
}


def bump_standards_version(control):
    current_version = control["Standards-Version"]
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
