#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater

with ChangelogUpdater() as updater:
    block = updater.changelog[0]
    seen = set()
    if block.distributions == 'UNRELEASED':
        for i, change in enumerate(list(block.changes())):
            if not change.startswith('  * '):
                # TODO(jelmer): Support multi-line items
                continue
            if change in seen:
                del block._changes[i]
            seen.add(change)


print('Remove duplicate line from changelog.')
