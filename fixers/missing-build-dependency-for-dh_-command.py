#!/usr/bin/python3

from lintian_brush.control import (
    ControlUpdater,
    PkgRelation,
    parse_relations,
    add_dependency,
    is_relation_implied,
    )
from lintian_brush.lintian_overrides import override_exists
from lintian_brush.rules import Makefile, Rule, dh_invoke_get_with
import shlex
import sys

# TODO(jelmer): Read /usr/share/lintian/data/debhelper/dh_commands{,-manual}
COMMAND_TO_DEP = {
    'dh_python2': 'dh-python | dh-sequence-python2',
    'dh_python3': 'dh-python | dh-sequence-python3',
    'dh_haskell_depends': 'haskell-devscripts-minimal',
    'dh_installtex': 'tex-common',
}

ADDON_TO_DEP = {
    'python3': 'dh-python | dh-sequence-python3',
    'autotools_dev': 'autotools-dev',
    'autoreconf':
        'dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat',
    'bash_completion': 'bash-completion',
    'gir': 'gobject-introspection',
    'systemd': 'debhelper (>= 9.20160709~) | debhelper-compat | dh-systemd',
}

need = set()
tags = {}

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
                need.add(dep)
                tags[dep] = 'missing-build-dependency-for-dh_command'
            if executable == 'dh' or executable.startswith('dh_'):
                for addon in dh_invoke_get_with(command):
                    try:
                        dep = ADDON_TO_DEP[addon]
                    except KeyError:
                        pass
                    else:
                        need.add(dep)
                        tags[dep] = 'missing-build-dependency-for-dh-addon'


if not need:
    sys.exit(0)


fixed_tags = set()


with ControlUpdater() as updater:
    for deps in need:
        parsed = PkgRelation.parse(deps)
        build_deps = updater.source.get('Build-Depends', '')
        for unused1, existing, unused2 in parse_relations(build_deps):
            if is_relation_implied(parsed, existing):
                break
        else:
            updater.source['Build-Depends'] = add_dependency(build_deps, deps)
            fixed_tags.add(tags[deps])

print('Add missing build dependency on dh addon.')
print('Fixed-Lintian-Tags: %s' % ', '.join(fixed_tags))
