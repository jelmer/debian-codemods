#!/usr/bin/python3

import re

from lintian_brush.fixer import report_result, LintianIssue
from lintian_brush.lintian_overrides import (
    update_overrides,
    LintianOverride,
    load_renamed_tags,
    )

# Simple match of "$file (line $lineno)"
PURE_FLN_SUB = (
    r"^(?P<path>.+) \(line (?P<lineno>\d+)\)$", r"[\1:\2]")
PURE_FLN_WILDCARD_SUB = (
    r"^(?P<path>.+) \(line (?P<lineno>\d+)\)$", r"* [\1:\2]")


INFO_FIXERS = {
    "autotools-pkg-config-macro-not-cross-compilation-safe": PURE_FLN_WILDCARD_SUB,
    "debian-rules-parses-dpkg-parsechangelog": PURE_FLN_SUB,
}

linenos = []


def fix_info(path, lineno, override):
    if not override.info:
        return override
    try:
        fixer = INFO_FIXERS[override.tag]
    except KeyError:
        pass  # no rename
    else:
        if isinstance(fixer, tuple):
            info = re.sub(fixer[0], fixer[1], override.info)
        elif callable(fixer):
            info = fixer(info) or info
        else:
            raise TypeError(fixer)
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
    "Update lintian override info to new format on line %s." % ', '.join(map(str, linenos)))
