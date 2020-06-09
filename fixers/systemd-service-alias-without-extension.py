#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceEditor
import os

for path in systemd_service_files():
    base, required_ext = os.path.splitext(path)
    with SystemdServiceEditor(path) as updater:
        for i, alias in enumerate(updater.file['Unit']['Alias']):
            base, ext = os.path.splitext(alias)
            if ext != required_ext:
                updater.file['Unit']['Alias'][i] = base + required_ext

print('Use proper extensions in Alias in systemd files.')
print('Fixed-Lintian-Tags: systemd-service-alias-without-extension')
