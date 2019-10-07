#!/usr/bin/python3

from debian.copyright import License
from lintian_brush.copyright import update_copyright, NotMachineReadableError

typos = {
    'bsd-2': 'BSD-2-Clause',
    'bsd-3': 'BSD-3-Clause',
    'bsd-4': 'BSD-4-Clause',
    'agpl3': 'AGPL-3',
    'agpl3+': 'AGPL-3+',
    'lgpl2.1': 'LGPL-2.1',
    'lgpl2': 'LGPL-2.0',
    'lgpl3': 'LGPL-3.0',
}
for i in [1, 2, 3]:
    typos['gplv%d' % i] = 'GPL-%d' % i
    typos['gplv%d+' % i] = 'GPL-%d+' % i
    typos['gpl%d' % i] = 'GPL-%d' % i
    typos['gpl%d+' % i] = 'GPL-%d+' % i

renames = {}


def fix_shortname(copyright):
    for paragraph in copyright.all_paragraphs():
        if paragraph.license is None:
            continue
        try:
            new_name = typos[paragraph.license.synopsis]
        except KeyError:
            continue
        renames[paragraph.license.synopsis] = new_name
        paragraph.license = License(new_name, paragraph.license.text)


try:
    update_copyright(fix_shortname)
except (FileNotFoundError, NotMachineReadableError):
    pass

print("Fix invalid short license name in debian/copyright (%s)" % (
    ', '.join(['%s => %s' % (old, new) for (old, new) in renames.items()])))
print('Fixed-Lintian-Tags: invalid-short-name-in-dep5-copyright')
