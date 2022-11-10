#!/usr/bin/python3

from typing import Dict, List

from lintian_brush.fixer import (
    report_result,
    LintianIssue,
    linenos_to_ranges,
    shorten_path,
)
from lintian_brush.lintian_overrides import (
    update_overrides,
    LintianOverride,
    fix_override_info,
)


fixed_linenos: Dict[str, List[int]] = {}


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
        fixed_linenos.setdefault(path, []).append(lineno)
        return LintianOverride(
            package=override.package, archlist=override.archlist,
            type=override.type, tag=override.tag,
            info=info)
    return override


update_overrides(fix_info)

if len(fixed_linenos) == 0:
    pass
elif len(fixed_linenos) == 1:
    [(path, linenos)] = fixed_linenos.items()
    report_result(
        "Update lintian override info format in %s on line %s."
        % (shorten_path(path), ', '.join(linenos_to_ranges(linenos))))
else:
    report_result(
        "Update lintian override info to new format:",
        details=[
            "%s: line %s" % (path, ', '.join(linenos_to_ranges(linenos)))
            for (path, linenos) in fixed_linenos.items()])
