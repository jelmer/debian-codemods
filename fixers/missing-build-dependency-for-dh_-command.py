#!/usr/bin/python3

from lintian_brush.control import update_control, add_dependency, get_relation
from lintian_brush.lintian_overrides import override_exists
from lintian_brush.rules import Makefile, Rule, dh_invoke_get_with
import shlex
import sys

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
            except ValueError:
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
            fixed_tags.add(tags[deps])


update_control(source_package_cb=add_missing_build_deps)

print('Add missing build dependency on dh addon.')
print('Fixed-Lintian-Tags: %s' % ', '.join(fixed_tags))
