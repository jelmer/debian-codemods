#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceEditor

for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        unit = updater.file['Unit']
        try:
            unit.rename('BindTo', 'BindsTo')
        except KeyError:
            pass


print('Rename BindTo key to BindsTo in systemd files.')
print('Fixed-Lintian-Tags: systemd-service-file-refers-to-obsolete-bindto')
