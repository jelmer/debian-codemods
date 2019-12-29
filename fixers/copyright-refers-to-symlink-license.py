#!/usr/bin/python3

from functools import partial
import os
import re

from debian.copyright import License

from lintian_brush.copyright import CopyrightUpdater

SYNOPSIS_ALIAS = {}
updated = set()
tags = set()


def replace_symlink_path(synopsis, m):
    path = m.group(0)
    waslink = os.path.islink(path)
    synopsis = SYNOPSIS_ALIAS.get(synopsis, synopsis)
    newpath = '/usr/share/common-licenses/' + synopsis.rstrip('+')
    if not os.path.exists(newpath) or os.path.islink(newpath):
        return path
    if not newpath.startswith(path + '-'):
        return path
    updated.add(synopsis)
    if waslink:
        tags.add('copyright-refers-to-symlink-license')
    tags.add('copyright-refers-to-versionless-license-file')
    return newpath


with CopyrightUpdater() as updater:
    for para in updater.copyright.all_paragraphs():
        license = para.license
        if not license or not license.text:
            continue
        para.license = License(license.synopsis, re.sub(
            '/usr/share/common-licenses/([A-Za-z0-9.]+)',
            partial(replace_symlink_path, license.synopsis), license.text))


print('Refer to specific version of license %s.' % ', '.join(sorted(updated)))
print('Fixed-Lintian-Tags: ' + ', '.join(sorted(tags)))
