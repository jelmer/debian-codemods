#!/usr/bin/python3

from lintian_brush.control import (
    ControlUpdater,
    )
from lintian_brush.fixer import report_result

from debian.changelog import Changelog
from email.utils import parseaddr
import sys

TEAM_UPLOAD_LINE = '  * Team upload.'
uploader_emails = []


with ControlUpdater() as updater:
    for entry in updater.source.get('Uploaders', '').split(','):
        uploader_emails.append(parseaddr(entry)[1])


with open('debian/changelog', 'r') as f:
    cl = Changelog(f.read())

last_change = cl[0]
has_team_upload = (TEAM_UPLOAD_LINE in last_change.changes())
name, email = parseaddr(last_change.author)
if email not in uploader_emails or not has_team_upload:
    sys.exit(2)

i = last_change._changes.index(TEAM_UPLOAD_LINE)
del last_change._changes[i]
if i > 0 and last_change._changes[i-1] == '' and last_change._changes[i] == '':
    # Also remove the next line, if it's empty
    del last_change._changes[i]

with open('debian/changelog', 'w') as f:
    f.write(str(cl))

report_result(
    "Remove unnecessary Team Upload line in changelog.",
    fixed_lintian_tags=['unnecessary-team-upload'])
