#!/usr/bin/python3

from debmutate.deb822 import Deb822Editor
from lintian_brush.fixer import report_result
import sys

try:
    from Levenshtein import distance
except ImportError:
    sys.exit(2)

valid_field_names = {
    'Files', 'License', 'Copyright', 'Comment',
    'Upstream-Name', 'Format', 'Upstream-Contact',
    'Source', 'Upstream', 'Contact', 'Name'}

typo_fixed = set()
case_fixed = set()

try:
    with Deb822Editor('debian/copyright') as updater:
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
    pass


if case_fixed:
    kind = 'case' + ('s' if len(case_fixed) > 1 else '')
else:
    kind = ''
if typo_fixed:
    if case_fixed:
        kind += ' and '
    kind += 'typo' + ('s' if len(typo_fixed) > 1 else '')

fixed_str = ', '.join(
    ['%s => %s' % (old, new)
     for (old, new) in sorted(list(case_fixed) + list(typo_fixed))])

report_result(
    'Fix field name %s in debian/copyright (%s).' % (kind, fixed_str),
    fixed_lintian_tags=(
        ['field-name-typo-in-dep5-copyright'] if typo_fixed else []))
