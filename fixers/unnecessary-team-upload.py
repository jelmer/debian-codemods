#!/usr/bin/python3

import sys
from email.utils import parseaddr

from debmutate.changelog import ChangelogEditor

from lintian_brush.fixer import control, fixed_lintian_tag, report_result

TEAM_UPLOAD_LINE = "  * Team upload."
uploader_emails = []


with control as updater:
    for entry in updater.source.get("Uploaders", "").split(","):
        uploader_emails.append(parseaddr(entry)[1])


with ChangelogEditor() as updater:
    last_change = updater.changelog[0]
    if last_change.distributions != "UNRELEASED":
        sys.exit(0)
    has_team_upload = TEAM_UPLOAD_LINE in last_change.changes()
    name, email = parseaddr(last_change.author)
    if email not in uploader_emails or not has_team_upload:
        sys.exit(0)

    i = last_change._changes.index(TEAM_UPLOAD_LINE)
    del last_change._changes[i]
    if (
        i > 0
        and last_change._changes[i - 1] == ""
        and last_change._changes[i] == ""
    ):
        # Also remove the next line, if it's empty
        del last_change._changes[i]
        fixed_lintian_tag("source", "unnecessary-team-upload")

report_result("Remove unnecessary Team Upload line in changelog.")
