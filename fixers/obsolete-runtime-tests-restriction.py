#!/usr/bin/python3

import os
import sys

from lintian_brush.deb822 import Deb822Updater


DEPRECATED_RESTRICTIONS = ['needs-recommends']
removed_restrictions = []


if not os.path.exists('debian/tests/control'):
    sys.exit(0)

with Deb822Updater('debian/tests/control') as updater:
    for paragraph in updater.paragraphs:
        restrictions = paragraph.get('Restrictions', '').split(',')
        if restrictions == ['']:
            continue
        for i, restriction in enumerate(list(restrictions)):
            if restriction.strip() in DEPRECATED_RESTRICTIONS:
                del restrictions[i]
                removed_restrictions.append(restriction.strip())
        paragraph['Restrictions'] = ','.join(restrictions).lstrip()
        if not paragraph['Restrictions'].strip():
            del paragraph['Restrictions']


print('Drop deprecated restriction%s %s. See '
      'https://salsa.debian.org/ci-team/autopkgtest/tree/'
      'master/doc/README.package-tests.rst' % (
       's' if len(removed_restrictions) > 1 else '',
       ', ' .join(removed_restrictions)))
print('Fixed-Lintian-Tags: obsolete-runtime-tests-restriction')
if 'needs-recommends' in removed_restrictions:
    # This is Certainty: possible, since the package may actually rely on the
    # (direct? indirect?) recommends, in which case we'd want to add them to
    # Depends.
    print('Certainty: possible')
