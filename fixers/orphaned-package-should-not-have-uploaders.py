#!/usr/bin/python3

from email.utils import parseaddr
from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result


with ControlEditor() as updater:
    if ("Maintainer" in updater.source and
            parseaddr(updater.source["Maintainer"])[1] ==
            'packages@qa.debian.org'):
        try:
            del updater.source["Uploaders"]
        except KeyError:
            pass

report_result(
    "Remove uploaders from orphaned package.",
    fixed_lintian_tags=['uploaders-in-orphan'])
