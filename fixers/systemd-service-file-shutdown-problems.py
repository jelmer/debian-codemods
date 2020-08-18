#!/usr/bin/python3

from lintian_brush.systemd import SystemdServiceEditor, systemd_service_files
from lintian_brush.fixer import report_result


for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        try:
            if (updater.file['Unit']['DefaultDependencies'] == 'no' and
                    'shutdown.target' in updater.file['Unit']['Conflicts'] and
                    'shutdown.target' not in updater.file['Unit']['Before']):
                updater.file['Unit']['Before'].append('shutdown.target')
        except KeyError:
            pass

report_result(
    "Add Before=shutdown.target to Unit section.",
    fixed_lintian_tags=['systemd-service-file-shutdown-problems'])
