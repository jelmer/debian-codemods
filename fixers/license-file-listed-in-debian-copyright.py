#!/usr/bin/python3

from debmutate.copyright import CopyrightEditor, NotMachineReadableError
from lintian_brush.fixer import report_result
import sys
import re
from typing import List

deleted = set()
certainty = 'certain'
message = "Remove listed license files (%s) from copyright."

# regex taken from /usr/share/lintian/checks/debian/copyright.pm
re_license = re.compile(r'(^|/)(COPYING[^/]*|LICENSE)$')

try:
    with CopyrightEditor() as updater:
        for paragraph in updater.copyright.all_files_paragraphs():
            files: List[str] = list()
            # access the private member because of #960278
            for f in paragraph._RestrictedWrapper__data['Files'].splitlines():
                if re_license.search(f.strip()):
                    deleted.add(f.strip())
                else:
                    if files:
                        files.append(f)
                    else:
                        # First line, should not start with whitespaces.
                        files.append(f.strip())
            files_entry = "\n".join(files)
            if not files_entry.strip():
                updater.copyright._Copyright__paragraphs.remove(paragraph)
            elif files_entry != paragraph._RestrictedWrapper__data['Files']:
                paragraph._RestrictedWrapper__data['Files'] = files_entry

        if not deleted:
            sys.exit(0)
except (FileNotFoundError, NotMachineReadableError):
    pass
else:
    report_result(
        message % ', '.join(sorted(deleted)),
        fixed_lintian_tags=['license-file-listed-in-debian-copyright'],
        certainty=certainty)
