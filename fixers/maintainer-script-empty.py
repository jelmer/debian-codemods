#!/usr/bin/python3

from lintian_brush.fixer import report_result, LintianIssue

import os


def is_empty(path):
    with open(path, 'rb') as f:
        for line in f.readlines():
            line = line.strip()
            if not line:
                continue
            if line.startswith(b'#'):
                continue
            if line.startswith(b'set '):
                continue
            if line.startswith(b'exit '):
                continue
            return False
    return True


MAINTAINER_SCRIPTS = ['prerm', 'postinst', 'preinst', 'postrm']
removed = []

for entry in os.scandir('debian'):
    if entry.name in MAINTAINER_SCRIPTS:
        script = entry.name
        package = None
    elif '.' not in entry.name:
        continue
    else:
        parts = entry.name.rsplit('.', 1)
        if len(parts) != 2:
            continue
        package = parts[0]
        script = parts[1]
        if script not in MAINTAINER_SCRIPTS:
            continue
    if is_empty(entry.path):
        issue = LintianIssue(package, 'maintainer-script-empty', script)
        if issue.should_fix():
            removed.append((package, script))
            os.unlink(entry.path)
            issue.report_fixed()

report_result(
    'Remove empty maintainer scripts: ' +
    ', '.join('%s (%s)' % x for x in removed))
