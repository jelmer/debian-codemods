#!/usr/bin/python3

from contextlib import suppress

from debmutate.copyright import CopyrightEditor, NotMachineReadableError

from lintian_brush.fixer import LintianIssue, report_result

with suppress(
    FileNotFoundError, NotMachineReadableError
), CopyrightEditor() as editor:
    files_i = 0
    for i, paragraph in enumerate(editor.copyright.all_files_paragraphs()):
        if "Files" in paragraph:
            if paragraph["Files"] == "*" and files_i > 0:
                issue = LintianIssue(
                    "source",
                    "global-files-wildcard-not-first-paragraph-in-"
                    "dep5-copyright",
                )
                if issue.should_fix():
                    editor.insert(0, editor.pop(i))
                    issue.report_fixed()
            files_i += 1

report_result('Make "Files: *" paragraph the first in the copyright file.')
