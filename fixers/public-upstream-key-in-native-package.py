#!/usr/bin/python3

import os
import sys
from lintian_brush.fixer import (
    package_is_native,
    report_result,
    LintianIssue,
    opinionated,
    )

if not package_is_native():
    # Nothing to do
    sys.exit(0)

if not opinionated():
    sys.exit(0)

for name in ['debian/upstream/signing-key.asc']:
    if not os.path.exists(name):
        continue
    issue = LintianIssue(
        'source', 'public-upstream-key-in-native-package', ())
    if not issue.should_fix():
        continue
    issue.report_fixed()
    os.unlink(name)

if os.listdir('debian/upstream') == []:
    os.rmdir('debian/upstream')

report_result(
    'Remove upstream signing key in native source package.',
    certainty='certain')
