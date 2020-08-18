#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceEditor
from lintian_brush.fixer import report_result

DEPRECATED_TARGETS = ['syslog.target']

removed = []

for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        for target in DEPRECATED_TARGETS:
            try:
                updater.file['Unit']['After'].remove(target)
            except ValueError:
                pass
            else:
                removed.append((path, target))

removed.sort()

report_result(
    'Remove references to obsolete targets in systemd unit files: %s.' %
    ', '.join(['%s (%s)' % (filename, target)
               for (filename, target) in removed]),
    fixed_lintian_tags=['systemd-service-file-refers-to-obsolete-target'])
