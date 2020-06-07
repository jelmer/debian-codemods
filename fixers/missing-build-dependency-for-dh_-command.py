#!/usr/bin/python3

from debmutate.control import (
    ControlEditor,
    PkgRelation,
    parse_relations,
    add_dependency,
    is_relation_implied,
    )
from lintian_brush.fixer import report_result
from lintian_brush.lintian import read_debhelper_lintian_data_file
from lintian_brush.lintian_overrides import override_exists
from lintian_brush.rules import Makefile, Rule, dh_invoke_get_with
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


for path, sep in [
    ('/usr/share/lintian/data/common/dh_addons', '='),
    ('/usr/share/lintian/data/debhelper/dh_addons-manual', '||'),
        ]:
    with open(path, 'r') as f:
        ADDON_TO_DEP.update(read_debhelper_lintian_data_file(f, sep))


need = []

mf = Makefile.from_path('debian/rules')

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
                if override_exists(
                    'missing-build-dependency-for-dh_-command',
                        package='source', info='%s => %s' % (executable, dep)):
                    continue
                need.append(
                    (dep, ['missing-build-dependency-for-dh_-command']))
            if executable == 'dh' or executable.startswith('dh_'):
                for addon in dh_invoke_get_with(command):
                    try:
                        dep = ADDON_TO_DEP[addon]
                    except KeyError:
                        pass
                    else:
                        need.append(
                            (dep, ['missing-build-dependency-for-dh-addon']))


if not need:
    sys.exit(0)


fixed_tags = set()


with ControlEditor() as updater:
    for deps, tags in need:
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
            fixed_tags.update(tags)

report_result(
    'Add missing build dependency on dh addon.',
    fixed_lintian_tags=fixed_tags)
