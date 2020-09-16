#!/usr/bin/python3

from lintian_brush.xdg import DesktopEntryEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag

import os

paths = []

for name in os.listdir('debian'):
    if name.endswith('.desktop'):
        path = os.path.join('debian', name)
        with DesktopEntryEditor(path) as updater:
            if updater.get('Encoding') == 'UTF-8':
                del updater.entry['Encoding']
                paths.append(path)
                fixed_lintian_tag(
                    'source', 'desktop-entry-contains-encoding-key',
                    'XX:YY Encoding')
            # TODO(jelmer): if encoding is non-UTF-8, invoke iconv.


if len(paths) == 1:
    report_result(
        'Remove deprecated Encoding key from desktop file %s.' % paths[0])
else:
    report_result(
        'Remove deprecated Encoding key from desktop files: %s.' % (
            ', '.join(sorted(paths))))
