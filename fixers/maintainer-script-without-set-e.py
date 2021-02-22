#!/usr/bin/python3

from lintian_brush.fixer import report_result, fixed_lintian_tag

import os


SCRIPTS = ['preinst', 'prerm', 'postinst', 'config', 'postrm']


def replace_set_e(path):
    lines = []
    try:
        with open(path, 'rb') as f:
            lines = list(f.readlines())
    except (FileNotFoundError, IsADirectoryError):
        return

    if b'set -e\n' in lines:
        return

    # TODO(jelmer): Handle -e in combination with other flags.
    if lines[0] != b"#!/bin/sh -e\n":
        return
    lines[0] = b'#!/bin/sh\n'
    for i, line in enumerate(lines[1:]):
        if line == b'set -e\n':
            return
        if (not (line.startswith(b'#') or line == b'\n') or
                line.strip() == b'#DEBHELPER#'):
            fixed_lintian_tag(
                'source', 'maintainer-script-without-set-e',
                os.path.basename(path))
            if not lines[i-1].strip():
                lines.insert(i, b'set -e\n')
                lines.insert(i+1, b'\n')
            else:
                lines.insert(i, b'\n')
                lines.insert(i+1, b'set -e\n')
            break

    with open(path, 'wb') as f:
        f.writelines(lines)


for name in SCRIPTS:
    replace_set_e(os.path.join('debian', name))


report_result('Use set -e rather than passing -e on the shebang-line.')
