#!/usr/bin/python3

from debmutate._rules import RulesEditor
from debmutate.control import drop_dependency, get_relation
from lintian_brush.debhelper import add_sequence
from lintian_brush.fixer import LintianIssue, control, report_result

with control:
    for binary in control.binaries:
        try:
            rel = get_relation(binary.get("Depends", ""), "vim-addon-manager")
        except KeyError:
            continue
        issue = LintianIssue(binary, "obsolete-vim-addon-manager")
        if not issue.should_fix():
            continue
        binary["Depends"] = drop_dependency(
            binary["Depends"], "vim-addon-manager"
        )
        with RulesEditor() as rules:
            add_sequence(control, rules, "vim-addon")
        issue.report_fixed()


report_result("Migrate from vim-addon-manager to dh-vim-addon.")
