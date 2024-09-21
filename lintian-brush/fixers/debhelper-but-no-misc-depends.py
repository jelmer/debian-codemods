#!/usr/bin/python3

from debmutate.control import (
    add_dependency,
    parse_relations,
)

from lintian_brush.fixer import LintianIssue, control, report_result

uses_debhelper = False
misc_depends_added = []


def has_misc_depends(relations):
    for entry in parse_relations(relations):
        (head_whitespace, relation, tail_whitespace) = entry
        if any(r.name == "${misc:Depends}" for r in relation):
            return True
    return False


with control as updater:
    for entry in parse_relations(updater.source.get("Build-Depends", "")):
        (head_whitespace, relation, tail_whitespace) = entry
        if any(r.name in ("debhelper", "debhelper-compat") for r in relation):
            uses_debhelper = True
            break

    if uses_debhelper:
        for binary in updater.binaries:
            if has_misc_depends(binary.get("Depends", "")) or has_misc_depends(
                binary.get("Pre-Depends", "")
            ):
                continue
            else:
                issue = LintianIssue(
                    updater.source,
                    "debhelper-but-no-misc-depends",
                    info=binary["Package"],
                )
                if issue.should_fix():
                    binary["Depends"] = add_dependency(
                        binary.get("Depends", ""), "${misc:Depends}"
                    )
                    misc_depends_added.append(binary["Package"])
                    issue.report_fixed()


report_result(
    "Add missing ${{misc:Depends}} to Depends for {}.".format(", ".join(misc_depends_added))
)
