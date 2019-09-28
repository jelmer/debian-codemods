#!/usr/bin/python3

from lintian_brush.reformatting import check_generated_file

def rewrite_line(line):
    if not line.split(b'#', 1)[0].strip():
        return line
    if line.startswith(b' ') or line.startswith(b'\t'):
        return line
    (key, value) = line.split(b':', 1)
    if not value.strip():
        return line
    return b'%s: %s\n' % (key, value.lstrip().rstrip(b'\n'))


lines = []
changed = False
with open('debian/control', 'rb') as f:
    for oldline in f:
        newline = rewrite_line(oldline)
        if newline != oldline:
            changed = True
        lines.append(newline)

if changed:
    with open('debian/control', 'wb') as f:
        f.writelines(lines)
    print('Strip unusual field spacing from debian/control.')
    print('Fixed-Lintian-Tags: debian-control-has-unusual-field-spacing')
