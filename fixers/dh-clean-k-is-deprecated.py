#!/usr/bin/python3

from lintian_brush.rules import update_rules


def update_line(line, target):
    if line.strip() == b'dh_clean -k':
        return b'dh_prep'
    return line


update_rules(update_line)
print("debian/rules: Use dh_prep rather than \"dh_clean -k\".")
print("Fixed-Lintian-Tags: dh-clean-k-is-deprecated")
