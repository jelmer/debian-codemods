#!/usr/bin/python3

from lintian_brush.fixer import control, LintianIssue, report_result
from debmutate.control import get_relation, drop_dependency
from debmutate._rules import RulesEditor
from lintian_brush.debhelper import add_sequence

with control:
    for binary in control.binaries:
        try:
            rel = get_relation(binary.get('Depends', ''), 'vim-addon-manager')
        except KeyError:
            continue
        issue = LintianIssue(binary, 'obsolete-vim-addon-manager')
        if not issue.should_fix():
            continue
        binary['Depends'] = drop_dependency(binary['Depends'], 'vim-addon-manager')
        with RulesEditor() as rules:
            add_sequence(control, rules, 'vim-addon')
        issue.report_fixed()


report_result('Migrate from vim-addon-manager to dh-vim-addon.')
