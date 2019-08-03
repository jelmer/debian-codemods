#!/usr/bin/python3

from lintian_brush.control import update_control


def drop_ancient_python_versions(source):
    # Remove anything that involves python 2.6, 2.7, 3.3
    if "X-Python-Version" in source:
        if source["X-Python-Version"].strip().startswith(">= 2."):
            del source["X-Python-Version"]
    if ("X-Python3-Version" in source and
            source["X-Python3-Version"].strip().startswith(">=")):
        vers = source["X-Python3-Version"].split(">=")[1].strip()
        if vers in ("3.0", "3.1", "3.2", "3.3", "3.4"):
            del source["X-Python3-Version"]


update_control(source_package_cb=drop_ancient_python_versions)

print("Remove unnecessary X-Python{,3}-Version field in debian/control.")
print("Fixed-Lintian-Tags: ancient-python-version-field")
