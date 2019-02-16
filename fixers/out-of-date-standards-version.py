#!/usr/bin/python3

from lintian_brush.control import update_control


def bump_standards_version(control):
    control["Standards-Version"] = "4.3.0"


update_control(source_package_cb=bump_standards_version)

print('Update standards version, no changes needed.')
print('Certainty: certain')
print('Fixed-Lintian-Tags: out-of-date-standards-version')
