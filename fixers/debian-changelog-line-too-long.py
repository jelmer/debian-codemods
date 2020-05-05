#!/usr/bin/python3

from lintian_brush.changelog import (
    ChangelogUpdater,
    rewrap_changes,
    any_long_lines,
    )
from lintian_brush.fixer import report_result


updated = []
with ChangelogUpdater() as updater:
    for block in updater.changelog:
        if not any_long_lines(block.changes()):
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
