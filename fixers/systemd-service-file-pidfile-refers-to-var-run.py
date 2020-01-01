#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceUpdater


for path in systemd_service_files():
    with SystemdServiceUpdater(path) as updater:
        val = updater.file['Service']['PIDFile']
        if isinstance(val, str):
            updater.file['Service']['PIDFile'] = val.replace(
                "/var/run/", "/run/")


print("Replace /var/run with /run for the Service PIDFile.")
print("Fixed-Lintian-Tags: systemd-service-file-pidfile-refers-to-var-run")
