#!/usr/bin/python3

from lintian_brush.fixer import report_result
from lintian_brush.systemd import (
    systemd_service_files, SystemdServiceEditor, Undefined
    )


for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        old_pidfile = updater.file['Service']['PIDFile']
        if isinstance(old_pidfile, str):
            new_pidfile = old_pidfile.replace("/var/run/", "/run/")
            updater.file['Service']['PIDFile'] = new_pidfile
        for key in updater.file['Service']:
            val = updater.file['Service'][key]
            if isinstance(old_pidfile, Undefined) or old_pidfile not in val:
                continue
            updater.file['Service'][key] = val.replace(
                old_pidfile, new_pidfile)

report_result(
    "Replace /var/run with /run for the Service PIDFile.",
    fixed_lintian_tags=['systemd-service-file-refers-to-var-run'])
