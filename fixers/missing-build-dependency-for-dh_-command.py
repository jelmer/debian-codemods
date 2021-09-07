#!/usr/bin/python3

from debmutate.control import (
    PkgRelation,
    parse_relations,
    add_dependency,
    is_relation_implied,
    )
from lintian_brush.fixer import control, report_result, LintianIssue
from lintian_brush.lintian import read_debhelper_lintian_data_file
from debmutate._rules import Makefile, Rule, dh_invoke_get_with
import os
import shlex
import sys

COMMAND_TO_DEP = {}


if not os.path.isdir('/usr/share/lintian/data'):
    # lintian doesn't appear to be installed
    sys.exit(2)


for path, sep in [
    ('/usr/share/lintian/data/debhelper/dh_commands', '='),
    ('/usr/share/lintian/data/debhelper/dh_commands-manual', '||'),
        ]:
    with open(path, 'r') as f:
        COMMAND_TO_DEP.update(read_debhelper_lintian_data_file(f, sep))


ADDON_TO_DEP = {}


with open('/usr/share/lintian/data/common/dh_addons', 'r') as f:
    ADDON_TO_DEP.update(read_debhelper_lintian_data_file(f, '='))


need = []

try:
    mf = Makefile.from_path('debian/rules')
except FileNotFoundError:
    sys.exit(0)

for entry in mf.contents:
    if isinstance(entry, Rule):
        for command in entry.commands():
            if command.lstrip().startswith(b'#'):
                continue
            try:
                executable = shlex.split(command.decode('utf-8', 'replace'))[0]
            except (ValueError, IndexError):
                continue
            try:
                dep = COMMAND_TO_DEP[executable]
            except KeyError:
                pass
            else:
                issue = LintianIssue(
                    'source', 'missing-build-dependency-for-dh_-command',
                    '%s => %s' % (executable, dep))
                if not issue.should_fix():
                    continue
                need.append((dep, [issue]))
            if executable == 'dh' or executable.startswith('dh_'):
                for addon in dh_invoke_get_with(command):
                    try:
                        dep = ADDON_TO_DEP[addon]
                    except KeyError:
                        pass
                    else:
                        issue = LintianIssue(
                            'source', 'missing-build-dependency-for-dh-addon',
                            '%s => %s' % (addon, dep))
                        need.append((dep, [issue]))


if not need:
    sys.exit(0)


with control as updater:
    for deps, issues in need:
        parsed = PkgRelation.parse(deps)
        is_implied = False

        for field in ['Build-Depends', 'Build-Depends-Indep',
                      'Build-Depends-Arch']:
            for unused1, existing, unused2 in parse_relations(
                    updater.source.get(field, '')):
                if is_relation_implied(parsed, existing):
                    is_implied = True

        if not is_implied:
            build_deps = updater.source.get('Build-Depends', '')
            updater.source['Build-Depends'] = add_dependency(build_deps, deps)
            for issue in issues:
                issue.report_fixed()

report_result('Add missing build dependency on dh addon.')
