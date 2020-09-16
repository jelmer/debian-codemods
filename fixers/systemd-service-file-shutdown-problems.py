#!/usr/bin/python3

from lintian_brush.systemd import SystemdServiceEditor, systemd_service_files
from lintian_brush.fixer import report_result, fixed_lintian_tag


for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        if (updater.file['Unit']['DefaultDependencies'] == 'no' and
                'shutdown.target' in updater.file['Unit']['Conflicts'] and
                'shutdown.target' not in updater.file['Unit']['Before']):
            updater.file['Unit']['Before'].append('shutdown.target')
            fixed_lintian_tag(
                'source', 'systemd-service-file-shutdown-problems', path)

report_result("Add Before=shutdown.target to Unit section.")
