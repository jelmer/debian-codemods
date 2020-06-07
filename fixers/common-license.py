#!/usr/bin/python3

from debian.copyright import License, NotMachineReadableError
from debmutate.copyright import CopyrightEditor
from lintian_brush.fixer import report_result
from lintian_brush.licenses import (
    COMMON_LICENSES_DIR,
    FULL_LICENSE_NAME,
    )

import os
import re
import textwrap
from typing import Dict
from warnings import warn


# In reality, what debian ships as "/usr/share/common-licenses/BSD" is
# BSD-3-clause in SPDX.
SPDX_RENAMES = {
    'BSD': 'BSD-3-clause',
    }
CANONICAL_NAMES = {
    'CC0': 'CC0-1.0',
}
updated = set()
tags = set()
_common_licenses: Dict[str, str] = {}


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
                f.read())
        return _common_licenses[name]


_COMMON_LICENSES = [
    ('CC0-1.0', cached_common_license('CC0-1.0').replace('Legal Code ', '')),
] + [(SPDX_RENAMES.get(name, name), cached_common_license(name))
     for name in os.listdir(COMMON_LICENSES_DIR)]


_BLURB = {
    'CC0-1.0': """\
To the extent possible under law, the author(s) have dedicated all copyright
and related and neighboring rights to this software to the public domain
worldwide. This software is distributed without any warranty.

You should have received a copy of the CC0 Public Domain Dedication along with
this software. If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.
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
""",
    'GPL-2+': """\
This package is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 2 of the License, or
(at your option) any later version.

This package is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <http://www.gnu.org/licenses/>
""",
    'GPL-3+': """\
This package is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 3 of the License, or
(at your option) any later version.

This package is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <http://www.gnu.org/licenses/>
""",
}


def find_common_license_from_fulltext(text):
    # Don't bother for anything that's short
    if len(text.splitlines()) < 15:
        return None
    text = normalize_license_text(text)
    text = drop_debian_file_reference(text) or text
    for shortname, fulltext in _COMMON_LICENSES:
        if license_text_matches(fulltext, text):
            return shortname


def drop_debian_file_reference(shorttext):
    try:
        i = shorttext.lower().index("on debian systems, ")
    except ValueError:
        return None
    return shorttext[:i].strip()


def debian_file_reference(name, filename):
    return '\n'.join(textwrap.wrap("""\
On Debian systems, the full text of the %(name)s
can be found in the file `/usr/share/common-licenses/%(filename)s'.
""" % {'name': name, 'filename': filename}, width=78))


def find_common_license_from_blurb(text):
    text = normalize_license_text(text)
    text_without_debian_reference = drop_debian_file_reference(text)
    for name, shorttext in _BLURB.items():
        if normalize_license_text(shorttext) in (
                text, text_without_debian_reference):
            return name


def canonical_license_id(license_id):
    # From the standard:
    #  For licenses that have multiple versions in use, the short name is
    #  formed from the general short name of the license family, followed by a
    #  dash and the version number. If the version number is omitted, the
    #  lowest version number is implied. When the license grant permits using
    #  the terms of any later version of that license, add a plus sign to the
    #  end of the short name. For example, the short name GPL refers to the GPL
    #  version 1 and is equivalent to GPL-1, although the latter is clearer and
    #  therefore preferred. If the package may be distributed under the GPL
    #  version 1 or any later version, use a short name of GPL-1+.
    #
    #  For SPDX compatibility, versions with trailing dot-zeroes are considered
    #  to be equivalent to versions without (e.g., “2.0.0” is considered equal
    #  to “2.0” and “2”).
    m = re.fullmatch(r'([A-Za-z0-9]+)(\-[0-9\.]+)?(\+)?', license_id)
    if not m:
        warn('Unable to get canonical name for %r' % license_id)
        return license_id
    version = (m.group(2) or '-1')[1:]
    while version.endswith('.0'):
        version = version[:-2]
    return '%s-%s%s' % (m.group(1), version, m.group(3) or '')


renames = {}


def replace_full_license(para):
    license = para.license
    license_matched = find_common_license_from_fulltext(license.text)
    if license_matched is None:
        if os.path.exists(os.path.join(COMMON_LICENSES_DIR, license.synopsis)):
            warn(
                'A common license shortname (%s) is used, but license '
                'text not recognized.' % license.synopsis, UserWarning)
        return
    # The full license text was found. Replace it with a blurb.
    canonical_id = canonical_license_id(license.synopsis)
    for shortname, blurb in _BLURB.items():
        if canonical_id == canonical_license_id(shortname):
            break
    else:
        if license.synopsis in SPDX_RENAMES:
            renames[license.synopsis] = SPDX_RENAMES[license.synopsis]
            return
        else:
            warn('Found full license text for %s, but unknown synopsis %s (%s)'
                 % (license_matched, license.synopsis, canonical_id))
        return
    if license_matched == 'Apache-2.0':
        tags.add('copyright-file-contains-full-apache-2-license')
    if license_matched.startswith('GFDL-'):
        tags.add('copyright-file-contains-full-gfdl-license')
    if license_matched.startswith('GPL-'):
        tags.add('copyright-file-contains-full-gpl-license')
    para.license = License(license.synopsis, blurb)
    return license_matched


def reference_common_license(para):
    license = para.license
    common_license = find_common_license_from_blurb(license.text)
    if not common_license:
        return
    if COMMON_LICENSES_DIR in license.text:
        return
    if para.comment is not None and COMMON_LICENSES_DIR in para.comment:
        return
    para.license = License(
        license.synopsis, license.text + '\n\n' + debian_file_reference(
            FULL_LICENSE_NAME.get(common_license, common_license),
            common_license))
    if common_license in ('Apache-2.0', 'Apache-2'):
        tags.add('copyright-not-using-common-license-for-apache2')
    elif common_license.startswith('GPL-'):
        tags.add('copyright-not-using-common-license-for-gpl')
    elif common_license.startswith('LGPL-'):
        tags.add('copyright-not-using-common-license-for-lgpl')
    elif common_license.startswith('GFDL-'):
        tags.add('copyright-not-using-common-license-for-gfdl')
    tags.add('copyright-does-not-refer-to-common-license-file')
    if license.synopsis != common_license:
        renames[license.synopsis] = common_license
    return common_license


try:
    with CopyrightEditor() as updater:
        for para in updater.copyright.all_paragraphs():
            license = para.license
            if not license or not license.text:
                continue
            replaced_license = replace_full_license(para)
            if replaced_license:
                updated.add(replaced_license)
            replaced_license = reference_common_license(para)
            if replaced_license:
                updated.add(replaced_license)
        for paragraph in updater.copyright.all_paragraphs():
            if not paragraph.license or not paragraph.license.synopsis:
                continue
            try:
                newsynopsis = renames[paragraph.license.synopsis]
            except KeyError:
                continue
            paragraph.license = License(newsynopsis, paragraph.license.text)
except (NotMachineReadableError, FileNotFoundError):
    pass


done = []
if updated:
    done.append(
        'refer to common license file for %s' % ', '.join(sorted(updated)))
if set(renames.values()) - set(updated):
    done.append('use common license names: ' + ', '.join(
        ['%s (was: %s)' % (new, old) for (old, new) in sorted(renames.items())
         if new not in updated]))


if done:
    report_result(
        done[0][0].capitalize() + ('; '.join(done) + '.')[1:],
        fixed_lintian_tags=tags)
