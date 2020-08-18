#!/usr/bin/python3

import os
from debmutate.changelog import (
    ChangelogEditor,
    rewrap_changes,
    any_long_lines,
    WIDTH,
    )
from lintian_brush.fixer import report_result, fixed_lintian_tag

updated = []


def wrap_block_lines(changes):
    if not any_long_lines(changes):
        return changes
    return rewrap_changes(changes)


def wrap_block(changelog, i):
    new_changes = wrap_block_lines(changelog[i].changes())
    if new_changes != changelog[i].changes():
        if i == 0:
            for lineno, change in enumerate(changelog[i].changes(), 2):
                if len(change) <= WIDTH:
                    continue
                # Lintian only warns about the first block.
                fixed_lintian_tag(
                    'source', 'debian-changelog-line-too-long',
                    info='line %d' % lineno)
        changelog[i]._changes = new_changes
        updated.append(changelog[i].version)
        return True
    return False


with ChangelogEditor() as updater:
    if 'CHANGELOG_THOROUGH' not in os.environ:
        wrap_block(updater.changelog, 0)
    else:
        for i in range(len(updater.changelog)):
            wrap_block(updater.changelog, i)


report_result(
    'Wrap long lines in changelog entries: %s.' % (
     ', '.join([str(v) for v in updated])))
