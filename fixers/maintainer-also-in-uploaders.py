#!/usr/bin/python3

import sys
from debmutate.control import delete_from_list

from lintian_brush.fixer import control, report_result, fixed_lintian_tag

# TODO(jelmer): support checking debcargo's maintainer/uploaders fields

try:
    with control as updater:
        if 'Uploaders' in updater.source:
            uploaders = updater.source['Uploaders'].split(',')
            maintainer = updater.source['Maintainer']
            if maintainer in [uploader.strip() for uploader in uploaders]:
                updater.source['Uploaders'] = delete_from_list(
                    updater.source['Uploaders'], maintainer)
                if not updater.source['Uploaders'].strip():
                    del updater.source['Uploaders']
                fixed_lintian_tag(
                    updater.source, 'maintainer-also-in-uploaders')
except FileNotFoundError:
    sys.exit(0)


report_result("Remove maintainer from uploaders.")
