#!/usr/bin/python3

from debmutate.watch import WatchEditor

from lintian_brush.fixer import report_result, fixed_lintian_tag

# TODO(jelmer): Remove dh_make pattern:

TEMPLATE_PATTERNS = [
    '^\s*#\s*(Example watch control file for uscan)'
    ]


with WatchEditor() as editor:
    for entry in editor.watch_file.entries:
        try:
            filenamemangle = entry.get_option('filenamemangle')
        except KeyError:
            continue
        if (filenamemangle == 
                's/.+\/v?(\d\S+)\.tar\.gz/<project>-$1\.tar\.gz/'):
            entry.del_option('filenamemangle')
            fixed_lintian_tag(
                'source', 'debian-watch-contains-dh_make-template',
                '<project>')


report_result('Remove dh_make template from debian watch.')
