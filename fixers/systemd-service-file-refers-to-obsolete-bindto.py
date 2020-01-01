#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceUpdater

for path in systemd_service_files():
    with SystemdServiceUpdater(path) as updater:
        updater.file['Unit'].rename('BindTo', 'BindsTo')


print('Rename BindTo key to BindsTo in systemd files.')
print('Fixed-Lintian-Tags: systemd-service-file-refers-to-obsolete-bindto')
