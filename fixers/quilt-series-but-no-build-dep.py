#!/usr/bin/python3

import os
from typing import Optional

from debmutate.control import ensure_some_version
from lintian_brush.fixer import control, report_result, fixed_lintian_tag

format: Optional[str]
try:
    with open('debian/source/format', 'r') as f:
        format = f.read().strip()
except FileNotFoundError:
    format = None

if format != '3.0 (quilt)' and os.path.exists('debian/patches/series'):
    with control as updater:
        updater.source['Build-Depends'] = ensure_some_version(
            updater.source['Build-Depends'], 'quilt')
    if updater.changed:
        fixed_lintian_tag(updater.source, 'quilt-series-but-no-build-dep')


report_result('Add missing dependency on quilt.')
