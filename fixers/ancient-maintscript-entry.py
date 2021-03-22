#!/usr/bin/python3

from datetime import datetime, timedelta
import os
import logging
import email.utils
import sys

from debian.changelog import Version
from debmutate.changelog import ChangelogEditor
from debmutate.debhelper import MaintscriptEditor
from lintian_brush.fixer import source_package_name, report_result, upgrade_release


maintscripts = []
for entry in os.scandir('debian'):
    if not (entry.name == "maintscript" or entry.name.endswith(".maintscript")):
        continue
    maintscripts.append(entry.name)


# Determine the date for which versions created then should still be supported.
# This is a little bit tricky since versions uploaded at a particular date
# may not have made it into the release then.
from distro_info import DebianDistroInfo  # noqa: E402
try:
    [release] = [
        r for r in DebianDistroInfo().get_all('object')
        if r.codename.lower() == upgrade_release()]
except ValueError:
    date_threshold = None
else:
    date_threshold = release.release

if date_threshold is None:
    # Release has not yet or will never be released
    # Default to 5 years
    date_threshold = (datetime.now() - timedelta(days=5 * 365)).date()


cl_dates = []
with ChangelogEditor() as cl:
    for block in cl:
        try:
            dt = email.utils.parsedate_to_datetime(block.date)
        except (TypeError, ValueError):
            logging.warning(
                'Invalid date %r for %s', block.date, block.version)
            # parsedate_to_datetime is buggy and raises a TypeError
            # when the date is invalid.
            # We can't reliably check anymore :(
            sys.exit(2)
        cl_dates.append((block.version, dt))


def is_long_passed(version):
    for (cl_version, cl_dt) in cl_dates:
        if cl_version <= version and cl_dt.date() > date_threshold:
            return False
    return True


ret = []
for name in maintscripts:
    with MaintscriptEditor(os.path.join('debian', name)) as editor:
        remove = []
        for i, entry in enumerate(list(editor.lines)):
            if isinstance(entry, str):
                continue
            prior_version = getattr(entry, "prior_version", None)
            if prior_version is None:
                continue
            if is_long_passed(Version(prior_version)):
                remove.append(i)
        removed = []
        for i in reversed(remove):
            removed.append(editor.lines[i])
            del editor.lines[i]
        if removed:
            ret.append((os.path.join('debian', name), removed))

if len(ret) == 1:
    report_result('Remove %d obsolete maintscript entry.' % len(ret))
else:
    report_result('Remove %d obsolete maintscript entries.' % len(ret))
