#!/usr/bin/python3

from debmutate.control import (
    PkgRelation,
    parse_relations,
    add_dependency,
    is_relation_implied,
    )
from lintian_brush.fixer import control, report_result, LintianIssue
from lintian_brush.lintian import LINTIAN_DATA_PATH, dh_commands, dh_addons
from debmutate._rules import Makefile, Rule, dh_invoke_get_with
import os
import shlex
import sys

COMMAND_TO_DEP = {}


if not os.path.isdir(LINTIAN_DATA_PATH):
    # lintian doesn't appear to be installed
    sys.exit(2)


for command, info in dh_commands().items():
    COMMAND_TO_DEP[command] = ' | '.join(info['installed_by'])

# Copied from /usr/share/lintian/lib/Lintian/Check/Debhelper.pm
COMMAND_TO_DEP.update({
    'dh_apache2': 'dh-apache2 | apache2-dev',
    'dh_autoreconf_clean':
        'dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat',
    'dh_autoreconf':
        'dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat',
    'dh_dkms': 'dkms | dh-sequence-dkms',
    'dh_girepository': 'gobject-introspection | dh-sequence-gir',
    'dh_gnome': 'gnome-pkg-tools | dh-sequence-gnome',
    'dh_gnome_clean': 'gnome-pkg-tools | dh-sequence-gnome',
    'dh_lv2config': 'lv2core',
    'dh_make_pgxs': 'postgresql-server-dev-all | postgresql-all',
    'dh_nativejava': 'gcj-native-helper | default-jdk-builddep',
    'dh_pgxs_test': 'postgresql-server-dev-all | postgresql-all',
    'dh_python2': 'dh-python | dh-sequence-python2',
    'dh_python3': 'dh-python | dh-sequence-python3',
    'dh_sphinxdoc': 'sphinx | python-sphinx | python3-sphinx',
    'dh_xine': 'libxine-dev | libxine2-dev',
})


ADDON_TO_DEP = {}
for addon, info in dh_addons().items():
    ADDON_TO_DEP[addon] = ' | '.join(info['installed_by'])


ADDON_TO_DEP.update({
    # Copied from /usr/share/lintian/lib/Lintian/Check/Debhelper.pm
    'ada_library': 'dh-ada-library | dh-sequence-ada-library',
    'apache2': 'dh-apache2 | apache2-dev',
    'autoreconf':
        'dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat',
    'cli': 'cli-common-dev | dh-sequence-cli',
    'dwz': 'debhelper | debhelper-compat | dh-sequence-dwz',
    'installinitramfs':
        'debhelper | debhelper-compat | dh-sequence-installinitramfs',
    'gnome': 'gnome-pkg-tools | dh-sequence-gnome',
    'lv2config': 'lv2core',
    'nodejs': 'pkg-js-tools | dh-sequence-nodejs',
    'perl_dbi': 'libdbi-perl | dh-sequence-perl-dbi',
    'perl_imager': 'libimager-perl | dh-sequence-perl-imager',
    'pgxs': 'postgresql-server-dev-all | postgresql-all',
    'pgxs_loop':  'postgresql-server-dev-all | postgresql-all',
    'pypy': 'dh-python | dh-sequence-pypy',
    'python2': 'python2:any | python2-dev:any | dh-sequence-python2',
    'python3':
        'python3:any | python3-all:any | python3-dev:any | '
        'python3-all-dev:any | dh-sequence-python3',
    'scour':  'scour | python-scour | dh-sequence-scour',
    'sphinxdoc':
        'sphinx | python-sphinx | python3-sphinx | dh-sequence-sphinxdoc',
    'systemd':
        'debhelper (>= 9.20160709~) | debhelper-compat | '
        'dh-sequence-systemd | dh-systemd',
    'vim_addon': 'dh_vim-addon | dh-sequence-vim-addon',
})

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
                    f'{executable} => {dep}')
                if not issue.should_fix():
                    continue
                need.append((dep, [issue], 'command', executable))
            if executable == 'dh' or executable.startswith('dh_'):
                for addon in dh_invoke_get_with(command):
                    try:
                        dep = ADDON_TO_DEP[addon]
                    except KeyError:
                        pass
                    else:
                        issue = LintianIssue(
                            'source', 'missing-build-dependency-for-dh-addon',
                            f'{addon} => {dep}')
                        need.append((dep, [issue], 'addon', addon))


if not need:
    sys.exit(0)

changed = []

with control as updater:
    for deps, issues, kind, name in need:
        parsed = PkgRelation.parse(deps)
        is_implied = False

        if is_relation_implied(parsed, 'debhelper'):
            is_implied = True

        for field in ['Build-Depends', 'Build-Depends-Indep',
                      'Build-Depends-Arch']:
            for _unused1, existing, _unused2 in parse_relations(
                    updater.source.get(field, '')):
                if is_relation_implied(parsed, existing):
                    is_implied = True

        if not is_implied:
            build_deps = updater.source.get('Build-Depends', '')
            updater.source['Build-Depends'] = add_dependency(build_deps, deps)
            for issue in issues:
                issue.report_fixed()
            changed.append((deps, issue, kind, name))

if len(changed) == 1:
    (dep, issue, kind, name) = changed[0]
    report_result(
        f'Add missing build dependency on {dep} for {kind} {name}.')
else:
    report_result(
        'Add missing build dependencies:' +
        '\n'.join('* %s for %s %s'
                  % (dep, kind, name) for (dep, issue, kind, name) in changed))
