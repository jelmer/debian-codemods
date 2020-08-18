#!/usr/bin/python3
import os

from lintian_brush.fixer import report_result, fixed_lintian_tag

try:
    st = os.stat('debian/rules')
except FileNotFoundError:
    pass
else:
    if not (st.st_mode & 0o111):
        os.chmod('debian/rules', 0o755)
        fixed_lintian_tag('source', 'debian-rules-not-executable')


report_result('Make debian/rules executable.')
