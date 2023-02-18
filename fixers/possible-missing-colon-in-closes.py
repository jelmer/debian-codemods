#!/usr/bin/python3

from functools import partial
from debmutate.changelog import ChangelogEditor
from lintian_brush import min_certainty
from lintian_brush.fixer import (
    fixed_lintian_tag,
    net_access_allowed,
    meets_minimum_certainty,
    warn,
    fixed_lintian_tags,
    report_result,
    )
import re
import socket

certainty = 'certain'
debbugs = None


def valid_bug(package, bug):
    if not net_access_allowed():
        return None
    global debbugs
    if debbugs is None:
        from lintian_brush.debbugs import DebBugs
        _debbugs = DebBugs()
        try:
            _debbugs.connect()
        except ImportError:
            # No asynpcg?
            return None
        except socket.gaierror as e:
            warn('Unable to connect to debbugs: %s' % e)
            return None
        debbugs = _debbugs
    return debbugs.check_bug(package, bug)


def check_bug(package, bugno):
    valid = valid_bug(package, bugno)
    if valid is not None:
        return (valid, 'certain')
    # Let's assume valid, but downgrade certainty
    valid = True
    # Check number of digits; upstream projects don't often hit the 5-digit
    # bug numbers that Debian has.
    if len(str(bugno)) >= 5:
        certainty = 'likely'
    else:
        certainty = 'possible'
    return (valid, certainty)


def fix_close_colon(package, m):
    global certainty
    bugno = int(m.group('bug'))
    (valid, bug_certainty) = check_bug(package, bugno)
    if meets_minimum_certainty(bug_certainty) and valid:
        certainty = min_certainty([certainty, bug_certainty])
        fixed_lintian_tag(
            'all', "possible-missing-colon-in-closes",
            info='%s #%d' % (m.group('closes'), bugno))
        return '%s: #%d' % (m.group('closes'), bugno)
    else:
        return m.group(0)


def fix_close_typo(package, m):
    global certainty
    bugno = int(m.group('bug'))
    (valid, bug_certainty) = check_bug(package, bugno)
    if meets_minimum_certainty(bug_certainty) and valid:
        certainty = min_certainty([certainty, bug_certainty])
        fixed_lintian_tag(
            'all', 'misspelled-closes-bug',
            info='#%s' % m.group('bug'))
        return '{}s: #{}'.format(m.group('close'), m.group('bug'))
    else:
        return m.group(0)


with ChangelogEditor() as updater:
    for block in updater.changelog:
        for i, change in enumerate(block._changes):
            change = re.sub(
                r'(?<!partially )(?P<closes>closes) '
                r'#(?P<bug>[0-9]+)',
                partial(fix_close_colon, block.package), change,
                flags=re.IGNORECASE)
            change = re.sub(
                '(?P<close>close): #(?P<bug>[0-9]+)',
                partial(fix_close_typo, block.package), change,
                flags=re.IGNORECASE)
            block._changes[i] = change


if fixed_lintian_tags() == {'possible-missing-colon-in-closes'}:
    report_result("Add missing colon in closes line.", certainty=certainty)
elif fixed_lintian_tags() == {'misspelled-closes-bug'}:
    report_result("Fix misspelling of Close ⇒ Closes.", certainty=certainty)
else:
    report_result("Fix formatting of bug closes.", certainty=certainty)
