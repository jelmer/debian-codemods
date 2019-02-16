#!/usr/bin/python3


from lintian_brush.control import update_control


# Dictionary mapping source and target versions
upgrade_path = {
    "4.2.0": "4.2.1",
}


def bump_standards_version(control):
    current_version = control["Standards-Version"]
    while current_version in upgrade_path:
        current_version = upgrade_path[current_version]
    control["Standards-Version"] = current_version


update_control(source_package_cb=bump_standards_version)

print('Update standards version, no changes needed.')
print('Certainty: certain')
print('Fixed-Lintian-Tags: out-of-date-standards-version')
