#!/usr/bin/python3

from debmutate.control import ControlEditor
from email.utils import parseaddr
from lintian_brush.fixer import report_result

QA_MAINTAINER = "Debian QA Group <packages@qa.debian.org>"

with ControlEditor() as updater:
    try:
        email = parseaddr(updater.source["Maintainer"])[1]
    except KeyError:
        # No maintainer? Weird, but sure.
        pass
    else:
        if email == "packages@qa.debian.org":
            updater.source["Maintainer"] = QA_MAINTAINER

report_result(
    "Fix Debian QA group name.",
    fixed_lintian_tags=['wrong-debian-qa-group-name'])
