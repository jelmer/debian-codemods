#!/usr/bin/python3

import os

from lintian_brush.control import ensure_some_version, update_control

try:
    with open('debian/source/format', 'r') as f:
        format = f.read().strip()
except FileNotFoundError:
    format = None

if format != '3.0 (quilt)' and os.path.exists('debian/patches/series'):
    def add_quilt_dependency(source):
        source['Build-Depends'] = ensure_some_version(
            source['Build-Depends'], 'quilt')

    update_control(source_package_cb=add_quilt_dependency)


print('Add missing dependency on quilt.')
print('Fixed-Lintian-Tags: quilt-series-but-no-build-dep')
