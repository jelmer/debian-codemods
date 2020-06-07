#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    meets_minimum_certainty,
    report_result,
    trust_package,
    )
from lintian_brush.upstream_metadata import guess_upstream_metadata_items

current_certainty = None

with ControlEditor() as updater:
    if 'Homepage' not in updater.source:
        for datum in guess_upstream_metadata_items(
                '.', trust_package=trust_package()):
            if datum.field != 'Homepage':
                continue
            if not meets_minimum_certainty(datum.certainty):
                continue
            if current_certainty != 'certain':
                updater.source["Homepage"] = datum.value
                current_certainty = datum.certainty

report_result(
    'Fill in Homepage field.',
    fixed_lintian_tags=['no-homepage-field'],
    certainty=current_certainty)
