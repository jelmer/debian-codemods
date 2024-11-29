#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import LintianIssue, report_result

try:
    with open("debian/source/options") as f:
        oldlines = list(f.readlines())
except FileNotFoundError:
    sys.exit(0)


def drop_prior_comments(lines):
    while lines and lines[-1].strip().startswith("#"):
        lines.pop(-1)


dropped = set()

newlines = []
for lineno, line in enumerate(oldlines, 1):
    if line.lstrip().startswith("#"):
        newlines.append(line)
        continue
    try:
        (key, value) = line.split("=", 1)
    except ValueError:
        newlines.append(line)
    else:
        if key.strip() == "compression":
            issue = LintianIssue(
                "source",
                "custom-compression-in-debian-source-options",
                f"{line.rstrip('\n')} (line {lineno})",
            )
            if issue.should_fix():
                drop_prior_comments(newlines)
                dropped.add("custom source compression")
                issue.report_fixed()
                continue
        if key.strip() == "compression-level":
            issue = LintianIssue(
                "source",
                "custom-compression-in-debian-source-options",
                f"{line.rstrip('\n')} (line {lineno})"
            )
            if issue.should_fix():
                drop_prior_comments(newlines)
                dropped.add("custom source compression level")
                issue.report_fixed()
                continue
        newlines.append(line)

if newlines:
    with open("debian/source/options", "w") as f:
        f.writelines(newlines)
elif dropped:
    os.unlink("debian/source/options")

report_result("Drop {}.".format(", ".join(sorted(dropped))))
