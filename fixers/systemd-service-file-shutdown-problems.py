#!/usr/bin/python3

from lintian_brush.systemd import SystemdServiceEditor, systemd_service_files


for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        try:
            if (updater.file['Unit']['DefaultDependencies'] == 'no' and
                    'shutdown.target' in updater.file['Unit']['Conflicts'] and
                    'shutdown.target' not in updater.file['Unit']['Before']):
                updater.file['Unit']['Before'].append('shutdown.target')
        except KeyError:
            pass

print("Add Before=shutdown.target to Unit section.")
print("Fixed-Lintian-Tags: systemd-service-file-shutdown-problems")
