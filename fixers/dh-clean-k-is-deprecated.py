#!/usr/bin/python3

from lintian_brush.rules import update_rules
from lintian_brush.fixer import report_result


def update_line(line, target):
    if line.strip() == b'dh_clean -k':
        return b'dh_prep'
    return line


update_rules(update_line)
report_result(
    "debian/rules: Use dh_prep rather than \"dh_clean -k\".",
    fixed_lintian_tags=["dh-clean-k-is-deprecated"])
