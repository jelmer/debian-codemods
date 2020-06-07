#!/usr/bin/python3

import os
from debmutate.changelog import (
    ChangelogEditor,
    rewrap_changes,
    any_long_lines,
    )
from lintian_brush.fixer import report_result

fixed_lintian_tags = set()
updated = []


def wrap_block_lines(block):
    if not any_long_lines(block.changes()):
        return False
    old_changes = list(block._changes)
    new_changes = rewrap_changes(block._changes)
    if old_changes != new_changes:
        block._changes = new_changes
        return True
    else:
        return False


def wrap_block(changelog, i):
    if wrap_block_lines(changelog[i]):
        updated.append(changelog[i].version)
        if i == 0:
            # Lintian only warns about the first block.
            fixed_lintian_tags.add('debian-changelog-line-too-long')
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
     ', '.join([str(v) for v in updated])),
    fixed_lintian_tags=fixed_lintian_tags)
