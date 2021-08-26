#!/usr/bin/python3

from email.utils import parseaddr
from lintian_brush.fixer import control, report_result, fixed_lintian_tag

with control as editor:
    try:
        old_maintainer = editor.source['Maintainer']
    except KeyError:
        pass
    else:
        maint, email = parseaddr(old_maintainer)
        if email in (
                'python-modules-team@lists.alioth.debian.org',
                'python-modules-team@alioth-lists.debian.net',
                'python-apps-team@lists.alioth.debian.org'):
            editor.source['Maintainer'] = (
                'Debian Python Team <team+python@tracker.debian.org>')
            fixed_lintian_tag(
                editor.source, 'python-teams-merged', info=(old_maintainer, ))

report_result('Update maintainer email for merge of DPMT and PAPT.')
