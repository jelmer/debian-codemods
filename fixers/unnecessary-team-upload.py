#!/usr/bin/python3

from lintian_brush.control import (
    update_control,
    )

from debian.changelog import Changelog
from email.utils import parseaddr
import sys

TEAM_UPLOAD_LINE = '  * Team upload.'
uploader_emails = []


def check_uploaders(control):
    for entry in control.get('Uploaders', '').split(','):
        uploader_emails.append(parseaddr(entry)[1])


update_control(source_package_cb=check_uploaders)

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

print("Remove unnecesary Team Upload line in changelog.")
print("Fixed-Lintian-Tags: unnecessary-team-upload")
