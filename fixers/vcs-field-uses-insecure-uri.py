#!/usr/bin/python3

from lintian_brush.fixer import (
    control,
    net_access_allowed,
    report_result,
    LintianIssue,
    )
from lintian_brush.vcs import find_secure_vcs_url

updated = set()
lp_note = False


with control as updater:
    for key, value in updater.source.items():
        if not key.startswith('Vcs-'):
            continue
        if value.startswith('lp:'):
            lp_note = True
        newvalue = find_secure_vcs_url(
            value, net_access=net_access_allowed())
        if newvalue is None:
            # We can't find a secure version :(
            continue
        if newvalue == value:
            # The URL was already secure
            continue
        issue = LintianIssue(
            'source', "vcs-field-uses-insecure-uri",
            info='{} {}'.format(key, updater.source[key]))
        if issue.should_fix():
            updater.source[key] = newvalue
            updated.add(key)
            issue.report_fixed()


if len(updated) == 1:
    description = [
        "Use secure URI in Vcs control header %s." % list(updated)[0]]
else:
    description = [
        "Use secure URI in Vcs control headers: %s." %
        ', '.join(sorted(updated))]
if lp_note:
    description.append('')
    description.append(
        "The lp: prefix gets expanded to http://code.launchpad.net/ for "
        "users that are not logged in on some versions of Bazaar.")


report_result('\n'.join(description))
