#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater, changes_by_author

with ChangelogUpdater() as updater:
    block = updater.changelog[0]
    if block.distributions == 'UNRELEASED':
        seen = set()
        for (author, linenos, change) in changes_by_author(block.changes()):
            change = ''.join(change)
            if (author, change) in seen:
                for lineno in reversed(linenos):
                    del block._changes[lineno]
            seen.add((author, change))


print('Remove duplicate line from changelog.')
