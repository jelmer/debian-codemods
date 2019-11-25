#!/usr/bin/python3


import os


SCRIPTS = ['prerm', 'postinst', 'config']


def replace_set_e(path):
    lines = []
    try:
        with open(path, 'rb') as f:
            lines = list(f.readlines())
    except (FileNotFoundError, IsADirectoryError):
        return

    if b'set -e\n' in lines:
        return

    if lines[0] != b"#!/bin/sh -e\n":
        return
    lines[0] = b'#!/bin/sh\n'
    for i, line in enumerate(lines[1:]):
        if line == b'set -e\n':
            return
        if (not (line.startswith(b'#') or line == b'\n') or
                line.strip() == b'#DEBHELPER#'):
            lines.insert(i, b'\n')
            lines.insert(i+1, b'set -e\n')
            break

    with open(path, 'wb') as f:
        f.writelines(lines)


for name in SCRIPTS:
    replace_set_e(os.path.join('debian', name))


print('Use set -e rather than passing -e on the shebang-line.')
print('Fixed-Lintian-Tags: maintainer-script-without-set-e')
