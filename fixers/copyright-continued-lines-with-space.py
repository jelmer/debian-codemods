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
    if l[0:1] in (b'\t', b' '):
        return whitespace_prefix_length(l)
    else:
        key, value = l.split(b':', 1)
        return len(key) + 1 + whitespace_prefix_length(value)


EXPECTED_HEADER = (
    b'Format: '
    b'https://www.debian.org/doc/packaging-manuals/copyright-format/1.0')

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
            lines.append(line)
            prev_value_offset = value_offset(line)
except FileNotFoundError:
    pass
else:
    with open('debian/copyright', 'wb') as f:
        f.writelines(lines)

print('debian/copyright: Use spaces rather than tabs in continuation lines.')
