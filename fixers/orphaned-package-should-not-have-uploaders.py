#!/usr/bin/python3

from email.utils import parseaddr
from lintian_brush.control import update_control


def drop_uploaders(control):
    if parseaddr(control["Maintainer"])[1] != 'packages@qa.debian.org':
        return
    try:
        del control["Uploaders"]
    except KeyError:
        pass


update_control(source_package_cb=drop_uploaders)
print("Remove uploaders from orphaned package.")
print("Fixed-Lintian-Tags: orphaned-package-should-not-have-uploaders")
