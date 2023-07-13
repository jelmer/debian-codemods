#!/usr/bin/python3

from lintian_brush.fixer import fixed_lintian_tag, report_result
from lintian_brush.systemd import SystemdServiceEditor, systemd_service_files

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
                fixed_lintian_tag(
                    'source',
                    'systemd-service-file-refers-to-obsolete-target',
                    (path, target))

removed.sort()

report_result(
    'Remove references to obsolete targets in systemd unit files: %s.' %
    ', '.join([f'{filename} ({target})'
               for (filename, target) in removed]))
