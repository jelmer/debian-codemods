#!/usr/bin/python3

from debmutate.changelog import ChangelogEditor
from debmutate.control import (
    ControlEditor,
    )
from lintian_brush.fixer import report_result

from email.utils import parseaddr
import sys

TEAM_UPLOAD_LINE = '  * Team upload.'
uploader_emails = []


with ControlEditor() as updater:
    for entry in updater.source.get('Uploaders', '').split(','):
        uploader_emails.append(parseaddr(entry)[1])


with ChangelogEditor() as updater:
    last_change = updater.changelog[0]
    if last_change.distributions != 'UNRELEASED':
        sys.exit(0)
    has_team_upload = (TEAM_UPLOAD_LINE in last_change.changes())
    name, email = parseaddr(last_change.author)
    if email not in uploader_emails or not has_team_upload:
        sys.exit(0)

    i = last_change._changes.index(TEAM_UPLOAD_LINE)
    del last_change._changes[i]
    if (i > 0 and last_change._changes[i-1] == '' and
            last_change._changes[i] == ''):
        # Also remove the next line, if it's empty
        del last_change._changes[i]

report_result(
    "Remove unnecessary Team Upload line in changelog.",
    fixed_lintian_tags=['unnecessary-team-upload'])
