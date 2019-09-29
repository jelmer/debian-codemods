#!/usr/bin/python3

from debian.copyright import License
from lintian_brush.copyright import update_copyright, NotMachineReadableError

RENAMES = {
  'Creative Commons Attribution Share-Alike (CC-BY-SA) v3.0': 'CC-BY-SA-3.0',
  'public domain': 'public-domain',
  'apache 2.0': 'Apache-2.0',
}


def fix_spaces(copyright):
    for paragraph in copyright.all_paragraphs():
        if not paragraph.license:
            continue
        if ' ' not in paragraph.license.synopsis:
            continue
        if paragraph.license.synopsis not in RENAMES:
            continue
        ors = paragraph.license.synopsis.replace(' | ', ' or ').split(' or ')
        newsynopsis = ' or '.join([RENAMES.get(name, name) for name in ors])
        paragraph.license = License(newsynopsis, paragraph.license.text)


try:
    update_copyright(fix_spaces)
except NotMachineReadableError:
    pass

print('Replace spaces in short license names with dashes.')
print('Fixed-Lintian-Tags: space-in-std-shortname-in-dep5-copyright')
