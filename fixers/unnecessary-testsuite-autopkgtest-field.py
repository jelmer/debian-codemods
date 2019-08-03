#!/usr/bin/python3

from lintian_brush.control import update_control


def drop_unnecessary_autopkgtest(source):
    if "Testsuite" not in source:
        return
    if source["Testsuite"] == "autopkgtest":
        del source["Testsuite"]


update_control(source_package_cb=drop_unnecessary_autopkgtest)

print("Remove unnecessary 'Testsuite: autopkgtest' header.")
print("Fixed-Lintian-Tags: unnecessary-testsuite-autopkgtest-field")
