#!/usr/bin/python3

from lintian_brush.control import (
    ControlUpdater,
    get_relation,
    drop_dependency,
    add_dependency,
    )
from lintian_brush.fixer import report_result


with ControlUpdater() as updater:
    try:
        get_relation(
            updater.source.get('Build-Depends-Indep', ''),
            'libmodule-build-perl')
    except KeyError:
        pass
    else:
        updater.source['Build-Depends-Indep'] = drop_dependency(
            updater.source['Build-Depends-Indep'], 'libmodule-build-perl')
        updater.source['Build-Depends'] = add_dependency(
            updater.source.get('Build-Depends', ''), 'libmodule-build-perl')


report_result(
    'Move libmodule-build-perl from Build-Depends-Indep to Build-Depends.',
    fixed_lintian_tags=['libmodule-build-perl-needs-to-be-in-build-depend'])
