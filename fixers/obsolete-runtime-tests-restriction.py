#!/usr/bin/python3

import os
import sys

from debmutate.deb822 import Deb822Editor
from debmutate.control import delete_from_list

from lintian_brush.fixer import report_result


removed_restrictions = []


if not os.path.exists('debian/tests/control'):
    sys.exit(0)

KNOWN_OBSOLETE_RESTRICTIONS_PATH = (
    '/usr/share/lintian/data/testsuite/known-obsolete-restrictions')
DEPRECATED_RESTRICTIONS = []

try:
    with open(KNOWN_OBSOLETE_RESTRICTIONS_PATH, 'r') as f:
        for line in f:
            if line.startswith('#'):
                continue
            if not line.strip():
                continue
            DEPRECATED_RESTRICTIONS.append(line.strip())
except FileNotFoundError:
    sys.exit(2)

with Deb822Editor('debian/tests/control') as updater:
    for paragraph in updater.paragraphs:
        restrictions = paragraph.get('Restrictions', '').split(',')
        if restrictions == ['']:
            continue
        to_delete = []
        for i, restriction in enumerate(list(restrictions)):
            if restriction.strip() in DEPRECATED_RESTRICTIONS:
                to_delete.append(restriction.strip())
        if to_delete:
            removed_restrictions.extend(to_delete)
            paragraph['Restrictions'] = delete_from_list(
                paragraph['Restrictions'], to_delete)
            if not paragraph['Restrictions'].strip():
                del paragraph['Restrictions']


if 'needs-recommends' in removed_restrictions:
    # This is Certainty: possible, since the package may actually rely on the
    # (direct? indirect?) recommends, in which case we'd want to add them to
    # Depends.
    certainty = 'possible'
else:
    certainty = 'certain'


report_result(
    'Drop deprecated restriction%s %s. See '
    'https://salsa.debian.org/ci-team/autopkgtest/tree/'
    'master/doc/README.package-tests.rst' % (
       's' if len(removed_restrictions) > 1 else '',
       ', ' .join(removed_restrictions)),
    fixed_lintian_tags=['obsolete-runtime-tests-restriction'],
    certainty=certainty)
