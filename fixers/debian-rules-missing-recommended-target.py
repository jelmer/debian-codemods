#!/usr/bin/python3

from lintian_brush.fixer import control, report_result, LintianIssue
from lintian_brush.rules import update_rules


def get_archs():
    archs = set()

    # TODO(jelmer): Not really an 'update'
    with control as updater:
        for binary in updater.binaries:
            archs.add(binary['Architecture'])
    return archs


added = []


def process_makefile(mf):
    has_build_arch = bool(list(mf.iter_rules(b'build-arch', exact=False)))
    has_build_indep = bool(list(mf.iter_rules(b'build-indep', exact=False)))

    if has_build_arch and has_build_indep:
        return

    if any([line.lstrip(b' -').startswith(b'include ')
            for line in mf.dump_lines()]):
        # No handling of includes for the moment.
        return

    archs = get_archs()
    if not has_build_indep:
        issue = LintianIssue(
            'source', 'debian-rules-missing-recommended-target',
            info='build-indep')
        if issue.should_fix():
            added.append('build-indep')
            mf.add_rule(
                b'build-indep',
                components=([b'build'] if 'all' in archs else None))
            issue.report_fixed()
    if not has_build_arch:
        added.append('build-arch')
        issue = LintianIssue(
            'source', 'debian-rules-missing-recommended-target',
            info='build-arch')
        if issue.should_fix():
            mf.add_rule(
                b'build-arch',
                components=([b'build'] if (archs - set(['all'])) else None))
            issue.report_fixed()

    if not added:
        return

    try:
        phony_rule = list(mf.iter_rules(b'.PHONY'))[-1]
    except IndexError:
        return

    for c in added:
        phony_rule.append_component(c.encode())


update_rules(makefile_cb=process_makefile)

if len(added) == 1:
    report_result('Add missing debian/rules target %s.' % added[0])
else:
    report_result('Add missing debian/rules targets %s.' % ', '.join(added))
