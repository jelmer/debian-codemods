#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater, TextWrapper, WIDTH
from lintian_brush.fixer import report_result
import re


updated = []
initial_re = re.compile(r'^[  ]+[\+\-\*] ')


def rewrap_change(change):
    if not change:
        return change
    m = initial_re.match(change[0])
    if any([len(line) > WIDTH for line in change]) and m:
        wrapper = TextWrapper(m.group(0))
        return wrapper.wrap(
                '\n'.join(l[len(m.group(0)):] for l in change))
    else:
        return change


def rewrap_changes(changes):
    change = []
    indent = None
    for line in changes:
        m = initial_re.match(line)
        if m:
            yield from rewrap_change(change)
            change = [line]
            indent = len(m.group(0))
        elif change and line.startswith(' ' * indent):
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
