#!/usr/bin/python3

import sys

# Import convenience functions for reporting results and checking overrides
from lintian_brush.fixer import control, report_result, LintianIssue

from email.utils import parseaddr

PKG_PERL_EMAIL = 'pkg-perl-maintainers@lists.alioth.debian.org'
TESTSUITE_VALUE = 'autopkgtest-pkg-perl'


with control as e:
    # Parse the maintainer field and extract the email address.
    try:
        (name, email) = parseaddr(e.source['Maintainer'])
    except KeyError:
        sys.exit(2)
    if email != PKG_PERL_EMAIL:
        # Nothing to do here, it's not a pkg-perl-maintained package
        sys.exit(0)
    if e.source.get('Testsuite') == TESTSUITE_VALUE:
        sys.exit(0)
    issue = LintianIssue(
        e.source, 'team/pkg-perl/testsuite/no-testsuite-header', info="autopkgtest")
    if issue.should_fix():
        e.source['Testsuite'] = TESTSUITE_VALUE
        issue.report_fixed()

report_result(
    'Set Testsuite header for perl package.',
    certainty='certain')
