#!/usr/bin/python3

import os
import sys

from debmutate.patch import read_quilt_series

from lintian_brush.fixer import LintianIssue, report_result, warn
from lintian_brush.line_editor import LineEditor

patches_path = "debian/patches"
if not os.path.isdir(patches_path):
    sys.exit(0)

with open(os.path.join(patches_path, "series"), 'rb') as series:
    patches = list(read_quilt_series(series))

if not patches:
    sys.exit(0)

for patch in patches:
    if patch.name is None:
        continue
    path = os.path.join(patches_path, patch.name)

    try:
        with LineEditor(path) as f:
            for lineno, line in f:
                try:
                    (key, value) = line.split(":", 1)
                except ValueError:
                    # Not a key-value pair.
                    continue
                if key.strip() == "Origin":
                    # If the Origin field is present and looks like an email address, use it as the author.
                    issue = LintianIssue(
                        target="source",
                        info=f"[{path}]",
                        tag="dep3-format-patch-author-or-from-is-better",
                    )
                    if issue.should_fix():
                        if "@" in value:
                            value = value.strip()
                            f[lineno] = f"Author: {value}\n"
                        issue.report_fixed()
                    break
    except FileNotFoundError:
        warn(f"Patch {path} does not exist.")


report_result(
    "Use Author instead of Origin in patch headers.", certainty="confident"
)
