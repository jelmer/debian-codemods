#!/usr/bin/python3

from debmutate.deb822 import Deb822Editor
from lintian_brush.fixer import report_result, LintianIssue

try:
    with Deb822Editor(path='debian/copyright') as updater:
        for paragraph in updater.paragraphs:
            if 'Files' not in paragraph:
                continue
            if ',' not in paragraph['Files']:
                continue
            if '{' in paragraph['Files']:
                # Bash-style expansion?
                continue
            issue = LintianIssue(
                'source', 'comma-separated-files-in-dep5-copyright',
                'paragraph at line XX')
            if issue.should_fix():
                paragraph['Files'] = '\n' + '\n'.join(
                    ' ' + entry.strip()
                    for entry in paragraph['Files'].split(','))
                issue.report_fixed()

except FileNotFoundError:
    pass

report_result(
    "debian/copyright: Replace commas with whitespace to separate items "
    "in Files paragraph.")
