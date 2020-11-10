#!/usr/bin/python3

import sys

from debmutate.control import (
    guess_template_type,
    )
from debmutate.reformatting import (
    check_generated_file,
    GeneratedFile,
    )

from lintian_brush.fixer import report_result, fixed_lintian_tag


def rewrite_line(line):
    if not line.split(b'#', 1)[0].strip():
        return line
    if line.startswith(b' ') or line.startswith(b'\t'):
        return line
    (key, value) = line.split(b':', 1)
    if not value.strip():
        return line
    return b'%s: %s\n' % (key, value.lstrip().rstrip(b'\n'))


def fix_field_spacing(path):
    lines = []
    changed = False
    with open(path, 'rb') as f:
        for lineno, oldline in enumerate(f, 1):
            newline = rewrite_line(oldline)
            if newline != oldline:
                changed = True
                if path == 'debian/control':
                    fixed_lintian_tag(
                        'source', 'debian-control-has-unusual-field-spacing',
                        info='line %d' % lineno)
            lines.append(newline)
    if not changed:
        return False
    with open(path, 'wb') as f:
        f.writelines(lines)
    return True


try:
    check_generated_file('debian/control')
except GeneratedFile as e:
    if e.template_path:
        template_type = guess_template_type(e.template_path)
        if template_type is None:
            raise
        changed = fix_field_spacing(e.template_path)
        if changed:
            fix_field_spacing('debian/control')
    else:
        raise
except FileNotFoundError:
    sys.exit(0)
else:
    changed = fix_field_spacing('debian/control')

if changed:
    report_result('Strip unusual field spacing from debian/control.')
