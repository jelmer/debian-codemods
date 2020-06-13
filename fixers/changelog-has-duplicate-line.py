#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater, changes_by_author
from lintian_brush.fixer import report_result

with ChangelogUpdater() as updater:
    block = updater.changelog[0]
    to_delete = []
    if block.distributions == 'UNRELEASED':
        seen = {}
        for (author, linenos, change) in changes_by_author(block.changes()):
            change_combined = ''.join(change)
            if (author, change_combined) in seen:
                to_delete.extend(linenos)
            else:
                seen[(author, change_combined)] = linenos
    for lineno in reversed(to_delete):
        del block._changes[lineno]


report_result('Remove duplicate line from changelog.')
