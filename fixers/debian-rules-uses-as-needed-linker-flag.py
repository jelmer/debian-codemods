#!/usr/bin/python3

import shlex
from lintian_brush.fixer import report_result, LintianIssue
from debmutate._rules import update_rules


def check_global(line):
    if line.startswith(b'export '):
        (name, value) = line[len(b'export '):].split(b'=', 1)
        name = name.strip()
        value = value.strip()
        if name == b'DEB_LDFLAGS_MAINT_APPEND' and b'-Wl,--as-needed' in value:
            issue = LintianIssue('source', 'debian-rules-uses-as-needed-linker-flag', 'line X')
            if issue.should_fix():
                issue.report_fixed()
                args = shlex.split(value.decode())
                args.remove('-Wl,--as-needed')
                return b'export %s = %s' % (name, shlex.join(args).encode())
            return line
    return line


update_rules(global_line_cb=check_global)


report_result('Avoid explicitly specifying -Wl,--as-needed linker flag.')
