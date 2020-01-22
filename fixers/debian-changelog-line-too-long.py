#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater
from lintian_brush.fixer import report_result
import textwrap

WIDTH = 80

updated = []
wrapper = textwrap.TextWrapper(
    width=WIDTH,
    initial_indent='  * ',
    subsequent_indent='    ',
    break_long_words=False,
    break_on_hyphens=False)


def rewrap_change(change):
    if (any([len(line) > WIDTH for line in change]) and
            change[0].startswith('  * ')):
        return wrapper.wrap(''.join(change)[4:])
    else:
        return change


def rewrap_changes(changes):
    change = []
    for line in changes:
        if line.startswith('  * '):
            yield from rewrap_change(change)
            change = [line]
        elif line.startswith('    ') and change:
            change.append(line)
        else:
            yield from rewrap_change(change)
            change = []
            yield line


with ChangelogUpdater() as updater:
    for block in updater.changelog:
        if not any([len(change) > WIDTH for change in block.changes()]):
            continue
        old_changes = list(block._changes)
        new_changes = rewrap_changes(block._changes)
        if old_changes != new_changes:
            block._changes = new_changes
            updated.append(block.version)

report_result(
    'Wrap long lines in changelog entries: %s.' % (
     ', '.join([str(v) for v in updated])),
    fixed_lintian_tags=['debian-changelog-line-too-long'])
