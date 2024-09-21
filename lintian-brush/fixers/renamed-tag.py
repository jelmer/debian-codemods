#!/usr/bin/python3

from lintian_brush.fixer import LintianIssue, report_result
from lintian_brush.lintian_overrides import (
    LintianOverride,
    load_renamed_tags,
    update_overrides,
)

renames = load_renamed_tags()


def rename_override_tags(path, lineno, override):
    try:
        new_tag = renames[override.tag]
    except KeyError:
        pass  # no rename
    else:
        issue = LintianIssue(
            (override.type, override.package),
            "renamed-tag",
            f"{override.tag} => {new_tag}",
        )
        if issue.should_fix():
            issue.report_fixed()
            return LintianOverride(
                package=override.package,
                archlist=override.archlist,
                type=override.type,
                tag=new_tag,
                info=override.info,
            )
    return override


update_overrides(rename_override_tags)

report_result("Update renamed lintian tag names in lintian overrides.")
