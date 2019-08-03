#!/usr/bin/python3

from lintian_brush.control import update_control
from email.utils import parseaddr


def fix_qa_group_name(source):
    email = parseaddr(source["Maintainer"])[1]
    if email == "packages@qa.debian.org":
        source["Maintainer"] = "Debian QA Group <packages@qa.debian.org>"


update_control(source_package_cb=fix_qa_group_name)

print("Fix Debian QA group name.")
print("Fixed-Lintian-Tags: wrong-debian-qa-group-name")
