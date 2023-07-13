#!/usr/bin/python3

import os

from lintian_brush.fixer import fixed_lintian_tag, report_result
from lintian_brush.systemd import SystemdServiceEditor, systemd_service_files

for path in systemd_service_files():
    base, required_ext = os.path.splitext(path)
    with SystemdServiceEditor(path) as updater:
        for i, alias in enumerate(updater.file['Unit']['Alias']):
            base, ext = os.path.splitext(alias)
            if ext != required_ext:
                updater.file['Unit']['Alias'][i] = base + required_ext
                fixed_lintian_tag(
                    'source',
                    'systemd-service-alias-without-extension',
                    '%s' % path)


report_result('Use proper extensions in Alias in systemd files.')
