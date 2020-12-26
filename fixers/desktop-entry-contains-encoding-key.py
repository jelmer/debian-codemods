#!/usr/bin/python3

from lintian_brush.xdg import DesktopEntryEditor
from lintian_brush.fixer import report_result, LintianIssue

import os

paths = []

for name in os.listdir('debian'):
    if name.endswith('.desktop'):
        path = os.path.join('debian', name)
        with DesktopEntryEditor(path) as updater:
            if updater.get('Encoding') == 'UTF-8':
                issue = LintianIssue(
                    'source', 'desktop-entry-contains-encoding-key',
                    'XX:YY Encoding')
                if issue.should_fix():
                    del updater.entry['Encoding']
                    paths.append(path)
                    issue.report_fixed()
            # TODO(jelmer): if encoding is non-UTF-8, invoke iconv.


if len(paths) == 1:
    report_result(
        'Remove deprecated Encoding key from desktop file %s.' % paths[0])
else:
    report_result(
        'Remove deprecated Encoding key from desktop files: %s.' % (
            ', '.join(sorted(paths))))
