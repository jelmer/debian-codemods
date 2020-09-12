#!/usr/bin/python3

import os
import re
from lintian_brush.fixer import report_result, fixed_lintian_tag

trailing_whitespace_re = re.compile(b'\\s*\n$')
trailing_space_re = re.compile(b'[ ]*\n$')


def strip_whitespace(line: bytes, strip_tabs=True):
    if strip_tabs:
        pat = trailing_whitespace_re
    else:
        pat = trailing_space_re
    return re.sub(pat, b'\n', line)


def file_strip_whitespace(
        path, strip_tabs=True, strip_trailing_empty_lines=True,
        delete_new_empty_line=False):
    newlines = []
    changed = False
    try:
        with open(path, 'rb') as f:
            for lineno, line in enumerate(f, 1):
                newline = strip_whitespace(line, strip_tabs=strip_tabs)
                if newline != line:
                    fixed_lintian_tag(
                        'source', 'trailing-whitespace',
                        info='%s (line %d)' % (path, lineno))
                    changed = True
                    if newline == b'\n' and delete_new_empty_line:
                        continue
                newlines.append(newline)
    except FileNotFoundError:
        return
    if strip_trailing_empty_lines:
        while newlines and newlines[-1] == b'\n':
            changed = True
            fixed_lintian_tag(
                'source', 'trailing-whitespace',
                info='%s (line %d)' % (path, len(newlines) - 1))
            newlines.pop(-1)
    if changed:
        with open(path, 'wb') as f:
            f.writelines(newlines)


file_strip_whitespace('debian/changelog', strip_tabs=True)
file_strip_whitespace('debian/rules', strip_tabs=False)
for entry in os.scandir('debian'):
    if (entry.name == 'control' or
       (entry.name.startswith('control.') and not entry.name.endswith('~'))):
        file_strip_whitespace(
            entry.path, strip_tabs=True, delete_new_empty_line=True)

report_result("Trim trailing whitespace.")
