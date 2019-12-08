#!/usr/bin/python3

import re
import sys


def expand_tabs(l, tabwidth=8):
    ret = []
    for i, c in enumerate(l):
        if l[i:i+1] == b'\t':
            # Round up to the next unit of tabwidth
            ret.extend([b' '] * (tabwidth - len(ret) % tabwidth))
        else:
            ret.append(l[i:i+1])
    return b''.join(ret)


def whitespace_prefix_length(l):
    return len(re.match(b'^\\s*', l).group(0))


def value_offset(l):
    if not l.strip():
        return None
    if l.startswith(b'#'):
        return None
    if l[0:1] in (b'\t', b' '):
        return whitespace_prefix_length(l)
    else:
        key, value = l.split(b':', 1)
        return len(key) + 1 + whitespace_prefix_length(value)


EXPECTED_HEADER = (
    b'Format: '
    b'https://www.debian.org/doc/packaging-manuals/copyright-format/1.0')


UNICODE_LINE_BREAK = "\u2028".encode('utf-8')
UNICODE_PARAGRAPH_SEPARATOR = "\u2029".encode('utf-8')


tabs_replaced = False
unicode_linebreaks_replaced = False

try:
    lines = []
    with open('debian/copyright', 'rb') as f:
        line = f.readline()
        if line.rstrip().rstrip(b'/') != EXPECTED_HEADER:
            sys.exit(0)
        lines.append(line)
        prev_value_offset = None
        for line in f:
            if line.startswith(b'\t'):
                for option in [
                        b' \t' + line[1:], b' \t' + line[2:],
                        b' ' * 8 + line[1:]]:
                    if value_offset(option) == prev_value_offset:
                        line = option
                        break
                else:
                    line = b' \t' + line[1:]
                tabs_replaced = True
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
    with open('debian/copyright', 'wb') as f:
        f.writelines(lines)

tags = set()
sys.stdout.write('debian/copyright: ')
if tabs_replaced:
    sys.stdout.write('use spaces rather than tabs to start continuation lines')
    if unicode_linebreaks_replaced:
        sys.stdout.write(', ')
    tags.add('tab-in-licence-text')
if unicode_linebreaks_replaced:
    sys.stdout.write('replace unicode linebreaks with regular linebreaks')
sys.stdout.write('.\n')
if tags:
    print('Fixed-Lintian-Tags: %s' % ', '.join(sorted(tags)))
