#!/usr/bin/python3

from lintian_brush.copyright import CopyrightUpdater, NotMachineReadableError
import sys

used = set()
defined = set()


try:
    with CopyrightUpdater() as updater:
        for paragraph in updater.copyright.all_files_paragraphs():
            if not paragraph.license:
                continue
            if paragraph.files:
                used.add(paragraph.license.synopsis)
        for paragraph in updater.copyright.all_paragraphs():
            if not paragraph.license:
                continue
            if paragraph.license.text:
                defined.add(paragraph.license.synopsis)

        extra_defined = (defined - used)
        extra_used = (used - defined)

        if extra_used:
            sys.stderr.write('Undefined licenses in copyright: %r' %
                             extra_used)

        if extra_defined and not extra_used:
            for paragraph in list(updater.copyright._Copyright__paragraphs):
                if paragraph.license.synopsis in extra_defined:
                    updater.copyright._Copyright__paragraphs.remove(paragraph)
except (FileNotFoundError, NotMachineReadableError):
    pass
else:
    print("Remove unused license definitions for %s." %
          ', '.join(extra_defined))
    print("Fixed-Lintian-Tags: unused-license-paragraph-in-dep5-copyright")
