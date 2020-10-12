#!/usr/bin/python3

import sys

# Import convenience functions for reporting results and checking overrides
from lintian_brush.fixer import report_result, fixed_lintian_tag, override_exists

from debmutate.control import ControlEditor
from email.utils import parseaddr

PKG_PERL_EMAIL = 'pkg-perl-maintainers@lists.alioth.debian.org'
URL_BASE = 'https://salsa.debian.org/perl-team/modules/packages'

with ControlEditor() as e:
    # Parse the maintainer field and extract the email address.
    (name, email) = parseaddr(e.source['Maintainer'])
    if email != PKG_PERL_EMAIL:
        # Nothing to do here, it's not a pkg-perl-maintained package
        sys.exit(0)
    # Iterate over all fields in the source package
    for field in list(e.source):
        if not field.lower().startswith('vcs-'):
            # Ignore non-Vcs fields
            continue
        if field.lower() not in ('vcs-git', 'vcs-browser'):
            if override_exists(e.source, 'team/pkg-perl/vcs/no-git', field):
                continue
            # Drop this field
            del e.source[field]
            fixed_lintian_tag(e.source, 'team/pkg-perl/vcs/no-git', field)

    for field, template in [
            ('Vcs-Git', URL_BASE + '/%s.git'),
            ('Vcs-Browser', URL_BASE + '/%s')]:
        if override_exists(
                e.source, 'team/pkg-perl/vcs/no-team-url', field):
            continue
        old_value = e.source.get(field)
        if old_value is not None and old_value.startswith(URL_BASE):
            continue

        e.source[field] = template % e.source['Source']
        # TODO(jelmer): Check that URLs actually exist, if net access is
        # allowed?
        fixed_lintian_tag(
            e.source, 'team/pkg-perl/vcs/no-team-url', old_value)


report_result(
    'Use standard Vcs fields for perl package.',
    certainty='certain')
