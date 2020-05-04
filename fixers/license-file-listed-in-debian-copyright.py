#!/usr/bin/python3

from lintian_brush.copyright import CopyrightUpdater, NotMachineReadableError
from lintian_brush.fixer import report_result
import sys
import re

deleted = set()
certainty = 'certain'
message = "Remove listed license files (%s) from copyright."

# regex taken from /usr/share/lintian/checks/debian/copyright.pm
re_license = re.compile(r'(^|/)(COPYING[^/]*|LICENSE)$')

try:
    with CopyrightUpdater() as updater:
        for paragraph in updater.copyright.all_files_paragraphs():
            files = list()
            for f in paragraph.files:
                if re_license.search(f):
                    deleted.add(f)
                else:
                    files.append(f)
            files = tuple(files)
            if not files:
                updater.copyright._Copyright__paragraphs.remove(paragraph)
            elif files != paragraph.files:
                paragraph.files = files

        if not deleted:
            sys.exit(0)
except (FileNotFoundError, NotMachineReadableError):
    pass
else:
    report_result(
        message % ', '.join(deleted),
        fixed_lintian_tags=['license-file-listed-in-debian-copyright'],
        certainty=certainty)
