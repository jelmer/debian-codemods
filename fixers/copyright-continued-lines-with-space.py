#!/usr/bin/python3

import re
import sys
from typing import List

from lintian_brush.fixer import LintianIssue, report_result


def expand_tabs(line, tabwidth=8):
    ret: List[bytes] = []
    for i, _c in enumerate(line):
        if line[i:i+1] == b'\t':
            # Round up to the next unit of tabwidth
            ret.extend([b' '] * (tabwidth - len(ret) % tabwidth))
        else:
            ret.append(line[i:i+1])
    return b''.join(ret)


def whitespace_prefix_length(line):
    m = re.match(b'^\\s*', line)
    if not m:
        return 0
    return len(m.group(0))


def value_offset(line):
    if not line.strip():
        return None
    if line.startswith(b'#'):
        return None
    if line[0:1] in (b'\t', b' '):
        return whitespace_prefix_length(line)
    else:
        try:
            key, value = line.split(b':', 1)
        except ValueError:
            return None
        else:
            return len(key) + 1 + whitespace_prefix_length(value)


EXPECTED_HEADER = (
    b'Format: '
    b'https://www.debian.org/doc/packaging-manuals/copyright-format/1.0')


UNICODE_LINE_BREAK = "\u2028".encode()
UNICODE_PARAGRAPH_SEPARATOR = "\u2029".encode()


tabs_replaced = False
unicode_linebreaks_replaced = False

try:
    lines = []
    with open('debian/copyright', 'rb') as f:
        line = f.readline()
        if line.rstrip().rstrip(b'/') != EXPECTED_HEADER:
            # Not a machine-readable copyright file
            sys.exit(0)
        lines.append(line)
        prev_value_offset = None
        for lineno, line in enumerate(f, start=2):
            if line.startswith(b'\t'):
                issue = LintianIssue(
                    'source', 'tab-in-license-text',
                    info='debian/copyright (paragraph at line %d)' % lineno)
                if issue.should_fix():
                    for option in [
                            b' \t' + line[1:], b' \t' + line[2:],
                            b' ' * 8 + line[1:]]:
                        if value_offset(option) == prev_value_offset:
                            line = option
                            break
                    else:
                        line = b' \t' + line[1:]
                    tabs_replaced = True
                    issue.report_fixed()
            if UNICODE_PARAGRAPH_SEPARATOR in line:
                # Not quite the same thing, but close enough..
                line = line.replace(
                    UNICODE_PARAGRAPH_SEPARATOR,
                    UNICODE_LINE_BREAK * 2)
            if UNICODE_LINE_BREAK in line:
                unicode_linebreaks_replaced = True
                parts = line.split(UNICODE_LINE_BREAK)
                # If the line is empty, replace it with a .
                parts = [p if p else b'.' for p in parts]
                parts = [parts[0]] + [b' ' + p for p in parts[1:]]
                line = b'\n'.join(parts)
            lines.append(line)
            prev_value_offset = value_offset(line)
except FileNotFoundError:
    pass
else:
    with open('debian/copyright', 'wb') as g:
        g.writelines(lines)

sys.stdout.write('debian/copyright: ')
if tabs_replaced:
    sys.stdout.write('use spaces rather than tabs to start continuation lines')
    if unicode_linebreaks_replaced:
        sys.stdout.write(', ')
if unicode_linebreaks_replaced:
    sys.stdout.write('replace unicode linebreaks with regular linebreaks')
sys.stdout.write('.\n')
report_result()
