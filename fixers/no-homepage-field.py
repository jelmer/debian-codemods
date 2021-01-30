#!/usr/bin/python3

from lintian_brush.fixer import (
    control,
    fixed_lintian_tag,
    meets_minimum_certainty,
    report_result,
    trust_package,
    )
from upstream_ontologist import guess_upstream_metadata_items

current_certainty = None

# TODO(jelmer): Support editing homepage field in debian/debcargo.toml

with control as updater:
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
                fixed_lintian_tag('source', 'no-homepage-field')


report_result('Fill in Homepage field.', certainty=current_certainty)
