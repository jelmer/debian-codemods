#!/usr/bin/python3

import sys

from lintian_brush.fixer import LintianIssue, control, report_result, vendor
from lintian_brush.lintian import known_binary_fields, known_source_fields

# See https://people.debian.org/~mpitt/autopkgtest/README.package-tests.html
valid_field_names = set()
valid_field_names.update(known_source_fields(vendor()))
valid_field_names.update(known_binary_fields(vendor()))

case_fixed = set()


try:
    with control as updater:
        for paragraph in updater.paragraphs:
            if paragraph.get('Source'):
                para_name = 'source'
            else:
                para_name = paragraph['Package']
            for field in list(paragraph):
                if field in valid_field_names:
                    continue
                for option in valid_field_names:
                    if option.lower() != field.lower():
                        continue
                    issue = LintianIssue(
                        updater.source, 'cute-field',
                        'debian/control@{} {} vs {}'.format(
                            para_name, field, option))
                    if issue.should_fix():
                        issue.report_fixed()
                        value = paragraph[field]
                        del paragraph[field]
                        paragraph[option] = value
                        case_fixed.add((field, option))
                        break
except FileNotFoundError:
    sys.exit(0)

if case_fixed:
    kind = 'case' + ('s' if len(case_fixed) > 1 else '')
else:
    kind = ''

fixed_str = ', '.join(
    [f'{old} ⇒ {new}'
     for (old, new) in sorted(list(case_fixed))])

report_result(
    f'Fix field name {kind} in debian/control ({fixed_str}).')
