#!/usr/bin/python3

import os
import subprocess
import sys

from hashlib import sha1

from lintian_brush.fixer import LintianIssue, report_result

issue = LintianIssue('source', 'newer-debconf-templates', info=())

if not issue.should_fix():
    sys.exit(0)

if not os.path.isdir('debian/po'):
    sys.exit(0)


def read_hashes():
    ret = {}
    for entry in os.scandir('debian/po'):
        with open(entry.path, 'rb') as f:
            ret[entry.path] = sha1(f.read()).hexdigest()
    return ret


def debconf_updatepo():
    subprocess.check_call(['debconf-updatepo'])


def update_timestamp(p, ts):
    import time
    ls = []
    with open(p, 'rb') as f:
        for line in f:
            if line.startswith(b'"POT-Creation-Date: '):
                tp = time.gmtime(ts)
                line = b'"POT-Creation-Date: %s\\n"\n' % (
                    time.strftime("%Y-%m-%d %H:%M+0000", tp).encode())
            ls.append(line)
    with open(p, 'wb') as f:
        f.writelines(ls)


if 'DEBCONF_GETTEXTIZE_TIMESTAMP' in os.environ:
    old_hashes = read_hashes()
    debconf_updatepo()
    new_hashes = read_hashes()
    for p in old_hashes:
        if old_hashes[p] == new_hashes[p]:
            continue
        update_timestamp(p, int(os.environ['DEBCONF_GETTEXTIZE_TIMESTAMP']))
else:
    debconf_updatepo()


issue.report_fixed()

report_result('Run debconf-updatepo after template changes.')
