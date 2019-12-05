#!/usr/bin/python3

import os
import sys

from lintian_brush.deb822 import update_deb822


DEPRECATED_RESTRICTIONS = ['needs-recommends']
removed_restrictions = []


def drop_deprecated_feature(paragraph):
    restrictions = paragraph.get('Restrictions', '').split(',')
    if restrictions == ['']:
        return
    for i, restriction in enumerate(list(restrictions)):
        if restriction.strip() in DEPRECATED_RESTRICTIONS:
            del restrictions[i]
            removed_restrictions.append(restriction.strip())
    paragraph['Restrictions'] = ','.join(restrictions).lstrip()
    if not paragraph['Restrictions'].strip():
        del paragraph['Restrictions']


if not os.path.exists('debian/tests/control'):
    sys.exit(0)

update_deb822(
    paragraph_cb=drop_deprecated_feature, path='debian/tests/control')


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
