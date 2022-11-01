#!/usr/bin/python3

from lintian_brush.fixer import (
    report_result,
    LintianIssue,
)
from lintian_brush.lintian_overrides import (
    update_overrides,
    LintianOverride,
    fix_override_info,
)


linenos = []


def fix_info(path, lineno, override):
    if not override.info:
        return override
    info = fix_override_info(override)
    if info != override.info:
        linenos.append(lineno)
    issue = LintianIssue(
        (override.type, override.package), 'mismatched-override',
        override.info + '[%s:%d]' % (path, lineno))
    if issue.should_fix():
        issue.report_fixed()
        return LintianOverride(
            package=override.package, archlist=override.archlist,
            type=override.type, tag=override.tag,
            info=info)
    return override


update_overrides(fix_info)

report_result(
    "Update lintian override info to new format on line %s."
    % ', '.join(map(str, linenos)))
