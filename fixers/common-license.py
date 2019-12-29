#!/usr/bin/python3

from debian.copyright import License
from lintian_brush.copyright import CopyrightUpdater

import os
import re


COMMON_LICENSES_DIR = '/usr/share/common-licenses'
updated = set()
_common_licenses = {}


def read_common_license(f):
    return f.read()


def normalize_license_text(text):
    return re.sub('[\n\t ]+', ' ', text).strip()


def license_text_matches(text1, text2):
    return text1 == text2


def cached_common_license(name):
    try:
        return _common_licenses[name]
    except KeyError:
        with open(os.path.join(COMMON_LICENSES_DIR, name), 'r') as f:
            _common_licenses[name] = normalize_license_text(
                read_common_license(f))
        return _common_licenses[name]


_COMMON_LICENSES = [
    ('CC0-1.0', cached_common_license('CC0-1.0')),
    ('CC0-1.0', cached_common_license('CC0-1.0').replace('Legal Code ', '')),
]


_BLURB = {
    'CC0-1.0': License('CC0-1.0', """\
To the extent possible under law, the author(s) have dedicated all copyright
and related and neighboring rights to this software to the public domain
worldwide. This software is distributed without any warranty.

You should have received a copy of the CC0 Public Domain Dedication along with
this software. If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.

On Debian systems, the complete text of the CC0 1.0 Universal license can be
found in "/usr/share/common-licenses/CC0-1.0".
"""),
    }


def find_common_license(text):
    text = normalize_license_text(text)
    for shortname, fulltext in _COMMON_LICENSES:
        if license_text_matches(fulltext, text):
            return shortname


with CopyrightUpdater() as updater:
    renames = {}
    for license_para in updater.copyright.all_license_paragraphs():
        license = license_para.license
        if not license.text:
            continue
        common_license = find_common_license(license.text)
        if common_license is None:
            continue
        renames[license.synopsis] = common_license
        license_para.license = _BLURB[common_license]
        updated.add(common_license)
    for paragraph in updater.copyright.all_paragraphs():
        if not paragraph.license or not paragraph.license.synopsis:
            continue
        try:
            newsynopsis = renames[paragraph.license.synopsis]
        except KeyError:
            continue
        paragraph.license = License(newsynopsis, paragraph.license.text)


print('Refer to common license file for %s.' % ', '.join(sorted(updated)))
print('Fixed-Lintian-Tags: copyright-does-not-refer-to-common-license-file')
