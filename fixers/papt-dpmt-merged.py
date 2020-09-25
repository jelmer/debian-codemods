#!/usr/bin/python3

from debmutate.control import ControlEditor
from email.utils import parseaddr
from lintian_brush.fixer import report_result, fixed_lintian_tag

with ControlEditor() as editor:
    maint, email = parseaddr(editor.source['Maintainer'])
    if email in (
            'python-modules-team@lists.alioth.debian.org',
            'python-apps-team@lists.alioth.debian.org'):
        editor.source['Maintainer'] = (
            'Debian Python Team <team+python@tracker.debian.org>')
        fixed_lintian_tag(editor.source, 'papt-dpmt-merged', info=())

report_result('Update maintainer email for merge of DPMT and PAPT.')
