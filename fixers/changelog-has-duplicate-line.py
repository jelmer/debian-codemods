#!/usr/bin/python3

from debmutate.changelog import ChangelogEditor, changes_sections
from lintian_brush.fixer import report_result

with ChangelogEditor() as updater:
    block = updater.changelog[0]
    to_delete = set()
    if block.distributions == 'UNRELEASED':
        seen = {}
        for (author, section_linenos, section_contents) in changes_sections(
                block.changes()):
            keep_section = False
            for entry in section_contents:
                change = ''.join([line for (lineno, line) in entry])
                if (author, change) in seen:
                    to_delete.update([lineno for (lineno, line) in entry])
                else:
                    seen[(author, change)] = entry
                    keep_section = True
            if not keep_section:
                to_delete.update(section_linenos)
    for lineno in sorted(to_delete, reverse=True):
        del block._changes[lineno]


report_result('Remove duplicate line from changelog.')
