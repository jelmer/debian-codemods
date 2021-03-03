#!/usr/bin/python3

from lintian_brush.fixer import control, report_result, LintianIssue
from lintian_brush.vcs import canonicalize_vcs_url


fields = set()


with control as updater:
    for name in updater.source:
        if not name.startswith("Vcs-"):
            continue
        new_value = canonicalize_vcs_url(
            name[len("Vcs-"):], updater.source[name])
        if new_value != updater.source[name]:
            issue = LintianIssue(
                updater.source, 'vcs-field-not-canonical',
                '%s %s' % (updater.source[name], new_value))
            if issue.should_fix():
                issue.report_fixed()
                updater.source[name] = new_value
                fields.add(name)

report_result("Use canonical URL in " + ', '.join(sorted(fields)) + '.')
