#!/usr/bin/python3

from lintian_brush.fixer import report_result, LintianIssue, control

with control as editor:
    for binary in control.binaries:
        try:
            desc = binary['Description']
        except KeyError:
            # Uhm, okay.
            continue
        lines = desc.splitlines(True)
        issue = LintianIssue(
            binary, 'extended-description-contains-empty-paragraph', ())
        if not issue.should_fix():
            continue
        if len(lines) > 1 and lines[1] == ' .\n':
            del lines[1]
        else:
            continue
        binary['Description'] = ''.join(lines)
        issue.report_fixed()

report_result('Remove empty leading paragraph in Description.')
