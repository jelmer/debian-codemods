#!/usr/bin/python3

from lintian_brush.fixer import (
    report_result,
    LintianIssue,
    linenos_to_ranges,
)
from lintian_brush.lintian_overrides import (
    update_overrides,
    LintianOverride,
    fix_override_info,
)


fixed_linenos = []


def fix_info(path, lineno, override):
    if not override.info:
        return override
    info = fix_override_info(override)
    if info == override.info:
        return override
    issue = LintianIssue(
        (override.type, override.package), 'mismatched-override',
        override.info + ' [%s:%d]' % (path, lineno))
    if issue.should_fix():
        issue.report_fixed()
        fixed_linenos.append(lineno)
        return LintianOverride(
            package=override.package, archlist=override.archlist,
            type=override.type, tag=override.tag,
            info=info)
    return override


update_overrides(fix_info)

report_result(
    "Update lintian override info to new format on line %s."
    % ', '.join(linenos_to_ranges(fixed_linenos)))
