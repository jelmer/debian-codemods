#!/usr/bin/python3

import shlex

from debmutate.debhelper import get_debhelper_compat_level
from lintian_brush.fixer import report_result, LintianIssue
from lintian_brush.rules import (
    dh_invoke_drop_argument,
    dh_invoke_drop_with,
    RulesEditor,
    )

removed_args = []
unnecessary_args = []
unnecessary_with = []


compat_version = get_debhelper_compat_level()
if compat_version and compat_version >= 10:
    unnecessary_args.append(b'--parallel')
    unnecessary_with.append(b'systemd')


def drop_unnecessary_args(line, target):
    if not line.startswith(b'dh ') and not line.startswith(b'dh_'):
        return line
    for arg in unnecessary_args:
        newline = dh_invoke_drop_argument(line, arg)
        if newline != line:
            issue = LintianIssue(
                'source', 'debian-rules-uses-unnecessary-dh-argument',
                info='dh ... %s' % arg.decode())
            if issue.should_fix():
                removed_args.append(arg)
                line = newline
                issue.report_fixed()
    for arg in unnecessary_with:
        newline = dh_invoke_drop_with(line, arg)
        if newline != line:
            issue = LintianIssue(
                'source', 'debian-rules-uses-unnecessary-dh-argument',
                info='dh ... -with=%s' % arg.decode())
            if issue.should_fix():
                removed_args.append(b'--with=%s' % arg)
                line = newline
                issue.report_fixed()
    return line


try:
    with RulesEditor() as updater:
        for rule in updater.makefile.iter_rules(b'%'):
            for command in rule.commands():
                if not command.startswith(b'dh'):
                    continue
                argv = shlex.split(command.decode())
                if argv[0] != 'dh':
                    continue
                for arg in argv:
                    if arg.startswith('--no-'):
                        actual = ('--' + arg[len('--no-'):])
                        if actual.encode() in unnecessary_args:
                            unnecessary_args.remove(actual.encode())
                    elif arg.startswith('--'):
                        actual = ('--no-' + arg[len('--'):])
                        if actual.encode() in unnecessary_args:
                            unnecessary_args.remove(actual.encode())
        updater.legacy_update(drop_unnecessary_args)
except FileNotFoundError:
    pass

report_result(
    'Drop unnecessary dh arguments: %s' %
    ', '.join([arg.decode() for arg in removed_args]))
