#!/usr/bin/python3

import shlex
from lintian_brush.fixer import report_result, LintianIssue
from debmutate._rules import update_rules


def check_global(origline):
    if origline.startswith(b'export '):
        prefix = b'export '
        line = origline[len(prefix):]
    else:
        prefix = b''
        line = origline
    try:
        (name, value) = line.split(b'=', 1)
    except ValueError:
        # Simple export of a variable, not an assignment
        return origline
    name = name.strip()
    value = value.strip()
    if name == b'DEB_LDFLAGS_MAINT_APPEND' and b'-Wl' in value:
        issue = LintianIssue(
            'source', 'debian-rules-uses-as-needed-linker-flag', 'line X')
        if issue.should_fix():
            issue.report_fixed()
            args = shlex.split(value.decode())
            orig_args = list(args)
            for i, arg in enumerate(args):
                if arg.startswith('-Wl'):
                    ld_args = arg.split(',')
                    try:
                        ld_args.remove('--as-needed')
                    except ValueError:
                        continue
                    if not ld_args[1:]:
                        args.remove('-Wl,--as-needed')
                    else:
                        args[i] = ','.join(ld_args)
            if not args:
                return None
            if orig_args != args:
                return prefix + b'%s = %s' % (name, shlex.join(args).encode())
        return origline
    return origline


try:
    update_rules(global_line_cb=check_global, drop_related_comments=True)  # type: ignore
except TypeError:  # debmutate < 0.62
    update_rules(global_line_cb=check_global)


report_result('Avoid explicitly specifying -Wl,--as-needed linker flag.')
