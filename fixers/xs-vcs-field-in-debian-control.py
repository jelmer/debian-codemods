#!/usr/bin/python3
from lintian_brush.control import update_control


def fix_xs_vcs_headers(control):
    for key in list(control):
        if key.startswith('XS-Vcs-'):
            control[key[3:]] = control[key]
            del control[key]


update_control(source_package_cb=fix_xs_vcs_headers)

print("Remove unnecessary XS- prefix for Vcs- fields in debian/control.")
print("Fixed-Lintian-Tags: xs-vcs-field-in-debian-control")
