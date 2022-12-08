#!/usr/bin/python3

from contextlib import suppress

from debmutate.watch import WatchEditor

from lintian_brush.fixer import report_result, LintianIssue

# TODO(jelmer): Remove dh_make pattern:

TEMPLATE_PATTERNS = [
    r'^\s*#\s*(Example watch control file for uscan)'
    ]


with suppress(FileNotFoundError), WatchEditor() as editor:
    if editor.watch_file:
        for entry in editor.watch_file.entries:
            try:
                filenamemangle = entry.get_option('filenamemangle')
            except KeyError:
                continue
            if (filenamemangle ==
                    r's/.+\/v?(\d\S+)\.tar\.gz/<project>-$1\.tar\.gz/'):
                issue = LintianIssue(
                    'source', 'debian-watch-contains-dh_make-template',
                    '<project>')
                if issue.should_fix():
                    entry.del_option('filenamemangle')
                    issue.report_fixed()

report_result('Remove dh_make template from debian watch.')
