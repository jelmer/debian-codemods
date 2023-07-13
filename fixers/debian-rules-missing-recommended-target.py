#!/usr/bin/python3

import os
import sys

from debmutate._rules import RulesEditor

from lintian_brush.fixer import LintianIssue, control, report_result


def get_archs():
    archs = set()

    # TODO(jelmer): Not really an 'update'
    with control as updater:
        for binary in updater.binaries:
            archs.add(binary['Architecture'])
    return archs


added = []


if not os.path.exists('debian/rules'):
    sys.exit(2)
with RulesEditor() as editor:
    has_build_arch = bool(list(
        editor.makefile.iter_rules(b'build-arch', exact=False)))
    has_build_indep = bool(list(
        editor.makefile.iter_rules(b'build-indep', exact=False)))

    if has_build_arch and has_build_indep:
        sys.exit(0)

    if any(line.lstrip(b' -').startswith(b'include ')
           for line in editor.makefile.dump_lines()):
        # No handling of includes for the moment.
        sys.exit(0)

    archs = get_archs()
    if not has_build_indep:
        issue = LintianIssue(
            'source', 'debian-rules-missing-recommended-target',
            info='build-indep')
        if issue.should_fix():
            added.append('build-indep')
            editor.makefile.add_rule(
                b'build-indep',
                components=([b'build'] if 'all' in archs else None))
            issue.report_fixed()
    if not has_build_arch:
        added.append('build-arch')
        issue = LintianIssue(
            'source', 'debian-rules-missing-recommended-target',
            info='build-arch')
        if issue.should_fix():
            editor.makefile.add_rule(
                b'build-arch',
                components=([b'build'] if (archs - {'all'}) else None))
            issue.report_fixed()

    if not added:
        sys.exit(0)

    try:
        phony_rule = list(editor.makefile.iter_rules(b'.PHONY'))[-1]
    except IndexError:
        pass
    else:
        for c in added:
            phony_rule.append_component(c.encode())


if len(added) == 1:
    report_result('Add missing debian/rules target %s.' % added[0])
else:
    report_result('Add missing debian/rules targets %s.' % ', '.join(added))
