#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceUpdater
import os

for path in systemd_service_files():
    base, required_ext = os.path.splitext(path)
    with SystemdServiceUpdater(path) as updater:
        for i, alias in enumerate(updater.file['Unit']['Alias']):
            base, ext = os.path.splitext(updater.file['Unit']['Alias'])
            if ext != required_ext:
                updater.file['Unit']['Alias'][i] = base + required_ext

print('Use proper extensions in Alias in systemd files.')
print('Fixed-Lintian-Tags: systemd-service-alias-without-extension')
