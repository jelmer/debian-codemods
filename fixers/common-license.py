#!/usr/bin/python3

from debian.copyright import License
from lintian_brush.copyright import CopyrightUpdater

import os
import re
from warnings import warn


COMMON_LICENSES_DIR = '/usr/share/common-licenses'
updated = set()
tags = set()
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
    ('Apache-2.0', cached_common_license('Apache-2.0')),
]


_BLURB = {
    'CC0-1.0': """\
To the extent possible under law, the author(s) have dedicated all copyright
and related and neighboring rights to this software to the public domain
worldwide. This software is distributed without any warranty.

You should have received a copy of the CC0 Public Domain Dedication along with
this software. If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.

On Debian systems, the complete text of the CC0 1.0 Universal license can be
found in "/usr/share/common-licenses/CC0-1.0".
""",
    'Apache-2.0': """\
Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

     http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

On Debian systems, the full text of the Apache License, Version 2.0
can be found in the file `/usr/share/common-licenses/Apache-2.0'.
""",
}


def find_common_license(text):
    text = normalize_license_text(text)
    for shortname, fulltext in _COMMON_LICENSES:
        if license_text_matches(fulltext, text):
            return shortname


def blurb_without_debian_reference(shorttext):
    i = shorttext.lower().index("on debian systems, ")
    if i == -1:
        return None
    return shorttext[:i].strip()


def find_common_blurb(text):
    text = normalize_license_text(text)
    for name, shorttext in _BLURB.items():
        if normalize_license_text(shorttext) == text:
            return name
        shorttext_without_debian_reference = blurb_without_debian_reference(
            shorttext)
        if shorttext_without_debian_reference is None:
            continue
        if normalize_license_text(shorttext_without_debian_reference) == text:
            return name


with CopyrightUpdater() as updater:
    renames = {}
    for license_para in updater.copyright.all_license_paragraphs():
        license = license_para.license
        if not license.text:
            continue
        common_license = find_common_license(license.text)
        if common_license is not None:
            license_para.license = License(
                common_license, _BLURB[common_license])
            updated.add(common_license)
            tags.add('copyright-does-not-refer-to-common-license-file')
        else:
            common_license = find_common_blurb(license.text)
            if common_license and COMMON_LICENSES_DIR not in license.text:
                license_para.license = License(
                    common_license, _BLURB[common_license])
                if common_license == 'Apache-2.0':
                    updated.add(common_license)
                    tags.add(
                        'copyright-should-refer-to-common-'
                        'license-file-for-apache-2')
            if common_license is None and os.path.exists(
                    os.path.join(COMMON_LICENSES_DIR, license.synopsis)):
                warn(
                    'A common license shortname (%s) is used, but license '
                    'text not recognized.' % license.synopsis, UserWarning)
        if common_license and license.synopsis != common_license:
            renames[license.synopsis] = common_license
    for paragraph in updater.copyright.all_paragraphs():
        if not paragraph.license or not paragraph.license.synopsis:
            continue
        try:
            newsynopsis = renames[paragraph.license.synopsis]
        except KeyError:
            continue
        paragraph.license = License(newsynopsis, paragraph.license.text)


print('Refer to common license file for %s.' % ', '.join(sorted(updated)))
print('Fixed-Lintian-Tags: %s' % ', '.join(sorted(tags)))
