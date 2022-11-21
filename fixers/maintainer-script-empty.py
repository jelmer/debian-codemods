#!/usr/bin/python3

from lintian_brush.fixer import report_result, LintianIssue, meets_minimum_certainty

import os


def is_empty(path):
    verdict = "empty"
    with open(path, 'rb') as f:
        for lineno, line in enumerate(f.readlines()):
            line = line.strip()
            if not line:
                continue
            if lineno == 0 and line.startswith(b"#!"):
                continue
            if line.startswith(b'#'):
                if line.lstrip(b'#') and line.strip() != b'#DEBHELPER#':
                    verdict = "some-comments"
                continue
            if line.startswith(b'set '):
                continue
            if line.startswith(b'exit '):
                continue
            return "not-empty"
    return verdict


MAINTAINER_SCRIPTS = ['prerm', 'postinst', 'preinst', 'postrm']
certainty = "certain"

removed = []

for entry in os.scandir('debian'):
    if entry.name in MAINTAINER_SCRIPTS:
        script = entry.name
        package = "source"
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
    verdict = is_empty(entry.path)
    if (verdict == "empty"
            or (verdict == "some-comments"
                and meets_minimum_certainty("likely"))):
        if verdict == "some-comments":
            certainty = "likely"
        issue = LintianIssue(package, 'maintainer-script-empty', script)
        if issue.should_fix():
            removed.append((package, script))
            os.unlink(entry.path)
            issue.report_fixed()

report_result(
    'Remove empty maintainer scripts: ' +
    ', '.join('%s (%s)' % x for x in removed),
    certainty=certainty)
