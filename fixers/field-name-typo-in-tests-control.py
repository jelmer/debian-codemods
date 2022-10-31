#!/usr/bin/python3

from debmutate.deb822 import Deb822Editor
from lintian_brush.fixer import report_result, vendor
from lintian_brush.lintian import known_tests_control_fields

import sys

try:
    from Levenshtein import distance
except ImportError:
    sys.exit(2)


# See https://people.debian.org/~mpitt/autopkgtest/README.package-tests.html
valid_field_names = set(known_tests_control_fields(vendor()))

case_fixed = set()
typo_fixed = set()


try:
    with Deb822Editor('debian/tests/control') as updater:
        for paragraph in updater.paragraphs:
            for field in paragraph:
                if field in valid_field_names:
                    continue
                for option in valid_field_names:
                    if distance(field, option) == 1:
                        value = paragraph[field]
                        del paragraph[field]
                        paragraph[option] = value
                        if option.lower() == field.lower():
                            case_fixed.add((field, option))
                        else:
                            typo_fixed.add((field, option))
                        break
except FileNotFoundError:
    sys.exit(0)

if case_fixed:
    kind = 'case' + ('s' if len(case_fixed) > 1 else '')
else:
    kind = ''
if typo_fixed:
    if case_fixed:
        kind += ' and '
    kind += 'typo' + ('s' if len(typo_fixed) > 1 else '')

fixed_str = ', '.join(
    ['%s ⇒ %s' % (old, new)
     for (old, new) in sorted(list(case_fixed) + list(typo_fixed))])

report_result(
    'Fix field name %s in debian/tests/control (%s).' % (kind, fixed_str))
