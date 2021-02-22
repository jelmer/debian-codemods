#!/usr/bin/python3

from debmutate.deb822 import Deb822Editor
from lintian_brush.fixer import report_result, LintianIssue, control

import sys

try:
    from Levenshtein import distance
except ImportError:
    sys.exit(2)


# See https://people.debian.org/~mpitt/autopkgtest/README.package-tests.html
valid_field_names = {
    'Homepage', 'Vcs-Git', 'Vcs-Bzr', 'Vcs-Svn', 'Vcs-Hg', 'Vcs-Browser',
    'Source', 'Package', 'Multi-Arch', 'Dm-Upload-Allowed', 'Pre-Depends',
    'Build-Depends', 'Build-Depends-Indep', 'Build-Depends-Arch',
    'Depends', 'X-Python3-Version', 'X-Debian-Abi'}

case_fixed = set()


try:
    with control as updater:
        for paragraph in updater.paragraphs:
            if paragraph.get('Source'):
                para_name = 'source'
            else:
                para_name = paragraph['Package']
            for field in paragraph:
                if field in valid_field_names:
                    continue
                for option in valid_field_names:
                    if option.lower() != field.lower():
                        continue
                    issue = LintianIssue(
                        updater.source, 'cute-field',
                        'debian/control@%s %s vs %s' % (
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
    ['%s => %s' % (old, new)
     for (old, new) in sorted(list(case_fixed))])

report_result(
    'Fix field name %s in debian/control (%s).' % (kind, fixed_str))
