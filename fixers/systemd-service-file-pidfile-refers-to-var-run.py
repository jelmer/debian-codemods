#!/usr/bin/python3

from lintian_brush.systemd import update_service


def replace_var_run(section, name, value):
    if section == b"Service" and name == b"PIDFile":
        return value.replace(b"/var/run/", b"/run/")
    return value


update_service(replace_var_run)
print("Replace /var/run with /run for the Service PIDFile.")
print("Fixed-Lintian-Tags: systemd-service-file-pidfile-refers-to-var-run")
