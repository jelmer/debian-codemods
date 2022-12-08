#!/usr/bin/python3

from contextlib import suppress
from functools import partial
import os
import re
from typing import Dict

from debian.copyright import License, NotMachineReadableError

from debmutate.copyright import CopyrightEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag

SYNOPSIS_ALIAS: Dict[str, str] = {}
updated = set()


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
        fixed_lintian_tag(
            'all', 'copyright-refers-to-symlink-license',
            info=path.lstrip('/'))
    fixed_lintian_tag(
        'all', 'copyright-refers-to-versionless-license-file',
        info=path.lstrip('/'))
    return newpath


with suppress(FileNotFoundError, NotMachineReadableError), \
        CopyrightEditor() as updater:
    for para in updater.copyright.all_paragraphs():
        license = para.license
        if not license or not license.text:
            continue
        changed_text = re.sub(
            '/usr/share/common-licenses/([A-Za-z0-9-.]+)',
            partial(replace_symlink_path, license.synopsis), license.text)
        if changed_text != license.text:
            para.license = License(license.synopsis, changed_text)


report_result(
    'Refer to specific version of license %s.' % ', '.join(sorted(updated)))
