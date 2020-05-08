#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater, changes_by_author
from lintian_brush.fixer import report_result

with ChangelogUpdater() as updater:
    block = updater.changelog[0]
    if block.distributions == 'UNRELEASED':
        seen = {}
        for (author, linenos, change) in changes_by_author(block.changes()):
            change = ''.join(change)
            if (author, change) in seen:
                for lineno in reversed(linenos):
                    del block._changes[lineno]
            else:
                seen[(author, change)] = linenos


report_result('Remove duplicate line from changelog.')
