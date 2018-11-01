#!/usr/bin/python3
from debian.changelog import Version
from io import StringIO
import re
from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_version,
    update_control,
    )


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
        control["Build-Depends"],
        "debhelper", Version("10"))
    control["Build-Depends"] = drop_dependency(
        control["Build-Depends"],
        "dh-autoreconf")


update_control(source_package_cb=bump_debhelper)

outf = StringIO()
with open('debian/rules', 'r') as f:
    for line in f:
        line = re.sub(" --with[ =]autoreconf( .+|)$", "\\1", line)
        line = re.sub(" --with[ =]autoreconf,", " --with=", line)
        line = re.sub(" --with[ =]([^ ]),autoreconf([ ,])", " --with=\\1\\2",
                      line)
        outf.write(line)

with open('debian/rules', 'w') as f:
    f.write(outf.getvalue())

print("Drop unnecessary dependency on dh-autoconf.")
print("Fixed-Lintian-Tags: useless-autoreconf-build-depends")
