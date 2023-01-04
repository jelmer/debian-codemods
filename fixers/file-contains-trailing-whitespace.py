#!/usr/bin/python3

import os
import re
from debmutate.reformatting import check_generated_file, GeneratedFile
from lintian_brush.fixer import report_result, LintianIssue

trailing_whitespace_re = re.compile(b'\\s*\n$')
trailing_space_re = re.compile(b'[ ]*\n$')


def strip_whitespace(line: bytes, strip_tabs=True) -> bytes:
    if strip_tabs:
        pat = trailing_whitespace_re
    else:
        pat = trailing_space_re
    return re.sub(pat, b'\n', line)


def file_strip_whitespace(
        path: str, strip_tabs: bool = True,
        strip_trailing_empty_lines: bool = True,
        delete_new_empty_line: bool = False) -> bool:
    newlines = []
    changed = False
    try:
        with open(path, 'rb') as f:
            for lineno, line in enumerate(f, 1):
                newline = strip_whitespace(line, strip_tabs=strip_tabs)
                if newline != line:
                    issue = LintianIssue(
                        'source', 'trailing-whitespace',
                        info='%s (line %d)' % (path, lineno))
                    if issue.should_fix():
                        issue.report_fixed()
                        changed = True
                        if newline == b'\n' and delete_new_empty_line:
                            continue
                newlines.append(newline)
    except FileNotFoundError:
        return False
    if strip_trailing_empty_lines:
        while newlines and newlines[-1] == b'\n':
            issue = LintianIssue(
                'source', 'trailing-whitespace',
                info='%s (line %d)' % (path, len(newlines) - 1))
            if issue.should_fix():
                issue.report_fixed()
                changed = True
                newlines.pop(-1)
            else:
                break
    if changed:
        with open(path, 'wb') as f:
            f.writelines(newlines)
    return changed


file_strip_whitespace('debian/changelog', strip_tabs=True)
file_strip_whitespace('debian/rules', strip_tabs=False)

try:
    check_generated_file('debian/control')
except GeneratedFile:
    changed = False
    for entry in os.scandir('debian'):
        if entry.name.startswith('control.') and not entry.name.endswith('~'):
            if not entry.name.endswith('.m4'):
                if file_strip_whitespace(
                        entry.path, strip_tabs=True,
                        delete_new_empty_line=True):
                    changed = True
    if changed:
        file_strip_whitespace('debian/control', strip_tabs=False)
else:
    file_strip_whitespace('debian/control', strip_tabs=False)

report_result("Trim trailing whitespace.")
