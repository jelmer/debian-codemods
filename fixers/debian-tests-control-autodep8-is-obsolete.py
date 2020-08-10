#!/usr/bin/python3

import os
import sys
from lintian_brush.fixer import fixed_lintian_tag, report_result

OLD_PATH = 'debian/tests/control.autodep8'
NEW_PATH = 'debian/tests/control'

if not os.path.exists(OLD_PATH):
    sys.exit(0)


fixed_lintian_tag(
    'source', 'debian-tests-control-autodep8-is-obsolete',
    info=OLD_PATH)

if not os.path.exists(NEW_PATH):
    os.rename(OLD_PATH, 'debian/tests/control')
    report_result("Rename obsolete path %s to %s." % (OLD_PATH, NEW_PATH))
else:
    with open(NEW_PATH, 'ab') as outf:
        outf.write(b'\n')
        with open(OLD_PATH, 'rb') as inf:
            outf.writelines(inf.readlines())
    os.unlink(OLD_PATH)
    fixed_lintian_tag(
        'source', 'debian-tests-control-and-control-autodep8',
        info='%s %s' % (OLD_PATH, NEW_PATH))
    report_result("Merge %s into %s." % (OLD_PATH, NEW_PATH))
