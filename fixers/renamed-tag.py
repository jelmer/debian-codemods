#!/usr/bin/python3

from lintian_brush import load_renamed_tags
from lintian_brush.fixer import report_result
from lintian_brush.lintian_overrides import (
    update_overrides,
    Override,
    )


renames = load_renamed_tags()


def rename_override_tags(override):
    if override.tag in renames:
        return Override(
            override.package, override.archlist, override.type,
            renames[override.tag], override.info)
    return override


update_overrides(rename_override_tags)

report_result(
    "Update renamed lintian tag names in lintian overrides.",
    fixed_lintian_tags=['renamed-tag'])
