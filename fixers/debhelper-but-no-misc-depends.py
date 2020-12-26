#!/usr/bin/python3

from debmutate.control import (
    add_dependency,
    parse_relations,
    ControlEditor,
    )
from lintian_brush.fixer import report_result, LintianIssue


uses_debhelper = False
misc_depends_added = []


with ControlEditor() as updater:
    for entry in parse_relations(updater.source.get("Build-Depends", '')):
        (head_whitespace, relation, tail_whitespace) = entry
        if any(r.name == 'debhelper' for r in relation):
            uses_debhelper = True

    if uses_debhelper:
        for binary in updater.binaries:
            for entry in parse_relations(binary.get("Depends", '')):
                (head_whitespace, relation, tail_whitespace) = entry
                if any(r.name == '${misc:Depends}' for r in relation):
                    break
            else:
                issue = LintianIssue(
                    updater.source, 'debhelper-but-no-misc-depends',
                    info=binary['Package'])
                if issue.should_fix():
                    binary["Depends"] = add_dependency(
                        binary.get("Depends", ''), "${misc:Depends}")
                    misc_depends_added.append(binary["Package"])
                    issue.report_fixed()


report_result(
    "Add missing ${misc:Depends} to Depends for %s." %
    ", ".join(misc_depends_added))
