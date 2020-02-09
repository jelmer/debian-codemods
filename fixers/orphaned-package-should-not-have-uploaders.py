#!/usr/bin/python3

from email.utils import parseaddr
from lintian_brush.control import ControlUpdater


with ControlUpdater() as updater:
    if ("Maintainer" in updater.source and
            parseaddr(updater.source["Maintainer"])[1] ==
            'packages@qa.debian.org'):
        try:
            del updater.source["Uploaders"]
        except KeyError:
            pass


print("Remove uploaders from orphaned package.")
print("Fixed-Lintian-Tags: orphaned-package-should-not-have-uploaders")
