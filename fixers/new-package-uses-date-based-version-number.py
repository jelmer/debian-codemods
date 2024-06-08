#!/usr/bin/python3

import re
import sys

from debmutate.changelog import ChangelogEditor, distribution_is_unreleased
from lintian_brush.fixer import LintianIssue, report_result

from debian.changelog import Version

with ChangelogEditor() as editor:
    if len(editor.changelog) != 1:
        sys.exit(0)

    block = editor.changelog[-1]

    if not distribution_is_unreleased(block.distributions):
        sys.exit(0)

    if not re.fullmatch("2[0-9]{7}", block.version.upstream_version):
        sys.exit(0)

    issue = LintianIssue(
        "source", "new-package-uses-date-based-version-number", None
    )
    if issue.should_fix():
        block.version = Version(f"0~{block.version}")
        issue.report_fixed()

report_result("Use version prefix for date-based versionioning.")
