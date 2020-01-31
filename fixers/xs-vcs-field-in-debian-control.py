#!/usr/bin/python3
from lintian_brush.control import ControlUpdater


with ControlUpdater() as updater:
    for key in list(updater.source):
        if key.startswith('XS-Vcs-'):
            updater.source[key[3:]] = updater.source[key]
            del updater.source[key]


print("Remove unnecessary XS- prefix for Vcs- fields in debian/control.")
print("Fixed-Lintian-Tags: xs-vcs-field-in-debian-control")
