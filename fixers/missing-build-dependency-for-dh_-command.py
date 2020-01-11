#!/usr/bin/python3

from lintian_brush.control import update_control, add_dependency, get_relation
from lintian_brush.lintian_overrides import override_exists
from lintian_brush.rules import Makefile, Rule
import shlex
import sys

command_to_dep = {
    'dh_python2': 'dh-python',
    'dh_python3': 'dh-python | dh-sequence-python3',
    'dh_haskell_depends': 'haskell-devscripts-minimal',
    'dh_installtex': 'tex-common',
}

need = set()

mf = Makefile.from_path('debian/rules')

for entry in mf.contents:
    if isinstance(entry, Rule):
        for command in entry.commands():
            executable = shlex.split(command.decode('utf-8', 'replace'))[0]
            try:
                dep = command_to_dep[executable]
            except KeyError:
                continue
            if override_exists(
                'missing-build-dependency-for-dh_-command',
                    package='source', info='%s => %s' % (executable, dep)):
                continue
            need.add(dep)

if not need:
    sys.exit(0)


def add_missing_build_deps(source):
    for deps in need:
        build_deps = source.get('Build-Depends', '')
        for dep in deps.split('|'):
            try:
                get_relation(build_deps, dep.strip())
            except KeyError:
                pass
            else:
                break
        else:
            source['Build-Depends'] = add_dependency(build_deps, deps)


update_control(source_package_cb=add_missing_build_deps)

print('Add missing build dependency for dh_ commands.')
print('Fixed-Lintian-Tags: missing-build-dependency-for-dh_command')
