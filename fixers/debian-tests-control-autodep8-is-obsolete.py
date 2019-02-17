#!/usr/bin/python3

import os
import sys

if not os.path.exists('debian/tests/control.autodep8'):
    sys.exit(0)


tags = ['debian-tests-control-autodep8-is-obsolete']

if not os.path.exists('debian/tests/control'):
    os.rename('debian/tests/control.autodep8', 'debian/tests/control')
    print(
        "Rename obsolete path debian/tests/control.autodep8 "
        "to debian/tests/control.")
else:
    with open('debian/tests/control', 'ab') as outf:
        outf.write(b'\n')
        with open('debian/tests/control.autodep8', 'rb') as inf:
            outf.writelines(inf.readlines())
    print("Merge debian/tests/control.autodep8 into debian/tests/control.")
    os.unlink("debian/tests/control.autodep8")
    tags.append('debian-tests-control-and-control-autodep8')

print("Fixed-Lintian-Tags: " + ', '.join(tags))
