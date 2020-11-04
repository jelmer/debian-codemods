#!/usr/bin/python3

from debmutate.watch import WatchEditor

from lintian_brush.fixer import report_result

WATCH_FILE_LATEST_VERSION = 4


with WatchEditor() as editor:
    if editor.watch_file.version != 4:
        editor.watch_file.version = 4


report_result(
    'Update watch file format version to %s.'
    % WATCH_FILE_LATEST_VERSION)
