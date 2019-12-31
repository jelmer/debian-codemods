#!/usr/bin/python3

from lintian_brush.systemd import update_service


def add_before_shutdown_target(section, name, value):
    if section == b"Unit" and name == b"Conflicts":
        value += b"\nBefore=shutdown.target"
    return value


update_service(add_before_shutdown_target)
print("Add Before=shutdown.target to Unit section.")
print("Fixed-Lintian-Tags: systemd-service-file-shutdown-problems")
