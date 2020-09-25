#!/usr/bin/python3

from debmutate.control import ControlEditor
from email.utils import parseaddr
from lintian_brush.fixer import report_result, fixed_lintian_tag

REPLACEMENTS = {
    'python-modules-team@lists.alioth.debian.org':
        ('old-dpmt-vcs',
         [('https://salsa.debian.org/python-team/modules/',
           'https://salsa.debian.org/python-team/packages/')]),
    'python-apps-team@lists.alioth.debian.org':
        ('old-papt-vcs',
         [('https://salsa.debian.org/python-team/applications/',
           'https://salsa.debian.org/python-team/packages/')]),
}

with ControlEditor() as editor:
    maint, email = parseaddr(editor.source['Maintainer'])
    changed_fields = set()
    try:
        tag, replacements = REPLACEMENTS[email]
    except KeyError:
        pass
    else:
        for field in [f for f in editor.source if f.startswith('Vcs-')]:
            url = editor.source[field]
            for old, new in replacements:
                url = url.replace(old, new)
            if url != editor.source[field]:
                editor.source[field] = url
                changed_fields.add(field)
                fixed_lintian_tag(editor.source, tag, info='')

    report_result('Update fields %s for maintainer %s.' % (
                  ', '.join(sorted(changed_fields)), maint))
