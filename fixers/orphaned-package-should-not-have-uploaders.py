#!/usr/bin/python3

from email.utils import parseaddr
from lintian_brush.control import ControlUpdater
from lintian_brush.fixer import report_result


with ControlUpdater() as updater:
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
