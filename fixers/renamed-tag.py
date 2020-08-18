#!/usr/bin/python3

from lintian_brush import load_renamed_tags
from lintian_brush.fixer import report_result, fixed_lintian_tag
from lintian_brush.lintian_overrides import (
    update_overrides,
    LintianOverride,
    )


renames = load_renamed_tags()


def rename_override_tags(override):
    if override.tag in renames:
        new_tag = renames[override.tag]
        fixed_lintian_tag(
            (override.type, override.package), 'renamed-tag',
            '%s => %s' % (override.tag, new_tag))
        return LintianOverride(
            package=override.package, archlist=override.archlist,
            type=override.type, tag=new_tag,
            info=override.info)
    return override


update_overrides(rename_override_tags)

report_result("Update renamed lintian tag names in lintian overrides.")
