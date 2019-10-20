#!/usr/bin/python3

import os
import json

from debian.copyright import License
from lintian_brush.copyright import update_copyright, NotMachineReadableError

RENAMES = {k.lower(): v for k, v in {
  'Creative Commons Attribution Share-Alike (CC-BY-SA) v3.0': 'CC-BY-SA-3.0',
  'Apache License Version 2.0': 'Apache-2.0',
}.items()}


# TODO(jelmer): Ideally we'd get the list of standard SPDX
REPLACE_SPACES = [
  'public-domain',
  'mit-style',
  'bsd-style',
]


def load_spdx_data():
    path = os.path.abspath(os.path.join(
        os.path.dirname(__file__), '..', 'spdx.json'))
    if not os.path.isfile(path):
        import pkg_resources
        path = pkg_resources.resource_filename(
            __name__, 'lintian-brush/spdx.json')
        if not os.path.isfile(path):
            # Urgh.
            path = '/usr/share/lintian-brush/spdx.json'
    with open(path, 'rb') as f:
        return json.load(f)


REPLACE_SPACES.extend(load_spdx_data()['license_ids'])
REPLACE_SPACES = set([license_id.lower() for license_id in REPLACE_SPACES])
for license_id in list(REPLACE_SPACES):
    if license_id.endswith('.0'):
        REPLACE_SPACES.add(license_id[:-2])


def fix_spaces(copyright):
    for paragraph in copyright.all_paragraphs():
        if not paragraph.license:
            continue
        if ' ' not in paragraph.license.synopsis:
            continue
        ors = paragraph.license.synopsis.replace(' | ', ' or ').split(' or ')
        names = []
        for name in ors:
            if name.lower() in RENAMES:
                name = RENAMES[name.lower()]
            elif name.replace(' ', '-').lower() in REPLACE_SPACES:
                name = name.replace(' ', '-')
            names.append(name)
        newsynopsis = ' or '.join(names)
        if newsynopsis != paragraph.license.synopsis:
            paragraph.license = License(newsynopsis, paragraph.license.text)


try:
    update_copyright(fix_spaces)
except (FileNotFoundError, NotMachineReadableError):
    pass

print('Replace spaces in short license names with dashes.')
print('Fixed-Lintian-Tags: space-in-std-shortname-in-dep5-copyright')
