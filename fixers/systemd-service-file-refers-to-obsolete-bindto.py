#!/usr/bin/python3

from lintian_brush.systemd import systemd_service_files, SystemdServiceEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag

for path in systemd_service_files():
    with SystemdServiceEditor(path) as updater:
        unit = updater.file['Unit']
        try:
            unit.rename('BindTo', 'BindsTo')
        except KeyError:
            pass
        else:
            fixed_lintian_tag(
                'source', 'systemd-service-file-refers-to-obsolete-bindto',
                path)


report_result('Rename BindTo key to BindsTo in systemd files.')
