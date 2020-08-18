#!/usr/bin/python3

from email.utils import parseaddr
from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    if ("Maintainer" in updater.source and
            parseaddr(updater.source["Maintainer"])[1] ==
            'packages@qa.debian.org'):
        try:
            del updater.source["Uploaders"]
        except KeyError:
            pass
        else:
            fixed_lintian_tag(updater.source, 'uploaders-in-orphan')


report_result("Remove uploaders from orphaned package.")
