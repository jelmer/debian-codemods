#!/usr/bin/python3

from debmutate.control import (
    ControlEditor,
    get_relation,
    drop_dependency,
    add_dependency,
    )
from lintian_brush.fixer import report_result, LintianIssue


with ControlEditor() as updater:
    try:
        get_relation(
            updater.source.get('Build-Depends-Indep', ''),
            'libmodule-build-perl')
    except KeyError:
        pass
    else:
        issue = LintianIssue(
            updater.source,
            'libmodule-build-perl-needs-to-be-in-build-depends', '')
        if issue.should_fix():
            updater.source['Build-Depends-Indep'] = drop_dependency(
                updater.source['Build-Depends-Indep'],
                'libmodule-build-perl')
            updater.source['Build-Depends'] = add_dependency(
                updater.source.get('Build-Depends', ''),
                'libmodule-build-perl')
            issue.report_fixed()


report_result(
    'Move libmodule-build-perl from Build-Depends-Indep to Build-Depends.')
