#!/usr/bin/python3


from debmutate.copyright import CopyrightEditor, NotMachineReadableError
from lintian_brush.fixer import report_result, LintianIssue


def swap_files_glob(copyright):
    files_i = 0
    for i, paragraph in enumerate(copyright._Copyright__paragraphs):
        if "Files" in paragraph:
            if paragraph["Files"] == "*" and files_i > 0:
                issue = LintianIssue(
                    'source',
                    'global-files-wildcard-not-first-paragraph-in-'
                    'dep5-copyright')
                if issue.should_fix():
                    copyright._Copyright__paragraphs.insert(
                        0, copyright._Copyright__paragraphs.pop(i))
                    issue.report_fixed()
            files_i += 1


try:
    with CopyrightEditor() as updater:
        swap_files_glob(updater.copyright)
except (FileNotFoundError, NotMachineReadableError):
    pass

report_result('Make "Files: *" paragraph the first in the copyright file.')
