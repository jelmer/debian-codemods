#!/usr/bin/python3

import sys
from email.utils import parseaddr

# Import convenience functions for reporting results and checking overrides
from lintian_brush.fixer import LintianIssue, control, report_result

PKG_PERL_EMAIL = 'pkg-perl-maintainers@lists.alioth.debian.org'
URL_BASE = 'https://salsa.debian.org/perl-team/modules/packages'

with control as e:
    # Parse the maintainer field and extract the email address.
    try:
        (name, email) = parseaddr(e.source['Maintainer'])
    except KeyError:
        sys.exit(2)
    if email != PKG_PERL_EMAIL:
        # Nothing to do here, it's not a pkg-perl-maintained package
        sys.exit(0)
    # Iterate over all fields in the source package
    for field in list(e.source):
        if not field.lower().startswith('vcs-'):
            # Ignore non-Vcs fields
            continue
        issue = LintianIssue(e.source, 'team/pkg-perl/vcs/no-git', info=field)
        if field.lower() not in ('vcs-git', 'vcs-browser'):
            if not issue.should_fix():
                continue
            # Drop this field
            del e.source[field]
            issue.report_fixed()

    for field, template in [
            ('Vcs-Git', URL_BASE + '/%s.git'),
            ('Vcs-Browser', URL_BASE + '/%s')]:
        old_value = e.source.get(field)
        issue = LintianIssue(
            e.source, 'team/pkg-perl/vcs/no-team-url',
            (field, old_value or ''))
        if not issue.should_fix():
            continue
        if old_value is not None and old_value.startswith(URL_BASE):
            continue

        e.source[field] = template % e.source['Source']
        # TODO(jelmer): Check that URLs actually exist, if net access is
        # allowed?
        issue.report_fixed()


report_result(
    'Use standard Vcs fields for perl package.',
    certainty='certain')
