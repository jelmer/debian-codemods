#!/usr/bin/python3

import sys
from lintian_brush.fixer import control, report_result, LintianIssue

changed = []


try:
    with control as updater:
        default_priority = updater.source.get('Priority')

        for binary in updater.binaries:
            if binary.get("Section") != 'libs':
                continue
            priority = binary.get('Priority', default_priority)
            if priority in ("required", "important", "standard"):
                issue = LintianIssue(
                    binary, 'excessive-priority-for-library-package',
                    info=priority)
                if issue.should_fix():
                    binary['Priority'] = 'optional'
                    changed.append(binary['Package'])
                    issue.report_fixed()
except FileNotFoundError:
    sys.exit(0)


report_result(
    'Set priority for library package{} {} to optional.'.format(
      's' if len(changed) > 1 else '', ', '.join(changed)))
