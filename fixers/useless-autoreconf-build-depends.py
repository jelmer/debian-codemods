#!/usr/bin/python3
from debian.changelog import Version
from io import StringIO
import re
import sys

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


changed = False
outf = StringIO()
with open('debian/rules', 'r') as f:
    for line in f:
        orig_line = line
        line = re.sub(" --with[ =]autoreconf( .+|)$", "\\1", line)
        line = re.sub(" --with[ =]autoreconf,", " --with=", line)
        line = re.sub(" --with[ =]([^ ]),autoreconf([ ,])", " --with=\\1\\2",
                      line)
        if orig_line != line:
            changed = True
        outf.write(line)


if not changed:
    sys.exit(2)

with open('debian/rules', 'w') as f:
    f.write(outf.getvalue())

update_control(source_package_cb=bump_debhelper)

print("Drop unnecessary dependency on dh-autoconf.")
print("Fixed-Lintian-Tags: useless-autoreconf-build-depends")
