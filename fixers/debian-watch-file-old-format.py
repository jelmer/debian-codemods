#!/usr/bin/python3

import sys
from contextlib import suppress

from debmutate.watch import WatchEditor

from lintian_brush.fixer import LintianIssue, report_result

OBSOLETE_WATCH_FILE_FORMAT = 2
WATCH_FILE_LATEST_VERSION = 4


with suppress(FileNotFoundError), WatchEditor() as editor:
    if editor.watch_file is None:
        sys.exit(0)
    if editor.watch_file.version >= WATCH_FILE_LATEST_VERSION:
        pass
    else:
        if editor.watch_file.version <= OBSOLETE_WATCH_FILE_FORMAT:
            tag = 'obsolete-debian-watch-file-standard'
        else:
            tag = 'older-debian-watch-file-standard'
        issue = LintianIssue('source', tag, '%d' % editor.watch_file.version)
        if issue.should_fix():
            editor.watch_file.version = WATCH_FILE_LATEST_VERSION
            issue.report_fixed()


report_result(
    'Update watch file format version to %s.'
    % WATCH_FILE_LATEST_VERSION)
