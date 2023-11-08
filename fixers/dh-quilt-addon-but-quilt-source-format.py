#!/usr/bin/python3

from contextlib import suppress

from debmutate._rules import (
    RulesEditor,
    dh_invoke_drop_with,
)

from lintian_brush.fixer import LintianIssue, report_result
from lintian_brush.patches import rules_find_patches_directory


def drop_quilt_with(line, target):
    newline = dh_invoke_drop_with(line, b"quilt")
    if line != newline:
        issue = LintianIssue(
            "source",
            "dh-quilt-addon-but-quilt-source-format",
            "dh ... --with quilt (line XX)",
        )
        if issue.should_fix():
            issue.report_fixed()
        else:
            newline = line
    return newline


with suppress(FileNotFoundError), open("debian/source/format") as f:
    if f.read().strip() == "3.0 (quilt)":
        with RulesEditor() as updater:
            if rules_find_patches_directory(updater.makefile) in (
                "debian/patches",
                None,
            ):
                updater.legacy_update(command_line_cb=drop_quilt_with)


report_result(
    "Don't specify --with=quilt, since package uses "
    "'3.0 (quilt)' source format."
)
