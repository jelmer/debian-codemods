#!/usr/bin/python3

from debian.copyright import License
from lintian_brush.copyright import update_copyright

typos = {
    'gplv2+': 'GPL-2+',
    'gpl2+': 'GPL-2+',
    }

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


update_copyright(fix_shortname)

print("Fix invalid short license name in debian/copyright (%s)" % (
    ', '.join(['%s => %s' % (old, new) for (old, new) in renames.items()])))
print('Fixed-Lintian-Tags: invalid-short-name-in-dep5-copyright')
