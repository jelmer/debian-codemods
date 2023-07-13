#!/usr/bin/python3

from lintian_brush.fixer import LintianIssue, report_result
from lintian_brush.lintian_overrides import update_overrides

REMOVED_TAGS = [
    'hardening-no-stackprotector',
    'maintainer-not-full-name',
    'uploader-not-full-name',
    'uploader-address-missing',
    'no-upstream-changelog',
    'copyright-year-in-future',
    'script-calls-init-script-directly',
    ]

# TODO(jelmer): Check if a tag matches a binary package name.


removed = []


def fix_malformed(path, lineno, override):
    if override.tag not in REMOVED_TAGS:
        return override
    issue = LintianIssue(
        (override.type, override.package), 'malformed-override',
        'Unknown tag %s in line %d' % (override.tag, lineno))
    if issue.should_fix():
        issue.report_fixed()
        removed.append(override.tag)
        return None
    return override


update_overrides(fix_malformed)

report_result(
    'Remove overrides for lintian tags that are no longer supported: %s' %
    ', '.join(removed))
