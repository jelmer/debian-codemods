#!/usr/bin/python3

from debmutate.control import (
    drop_dependency,
    ControlEditor,
    )
from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    for field in ['Build-Depends', 'Build-Depends-Indep']:
        try:
            updater.source[field] = drop_dependency(
                updater.source[field], "build-essential")
        except KeyError:
            pass
        else:
            fixed_lintian_tag(
                updater.source, 'build-depends-on-build-essential',
                field.lower())


report_result("Drop unnecessary dependency on build-essential.")
