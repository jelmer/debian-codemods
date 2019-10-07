#!/usr/bin/python3

from lintian_brush.copyright import update_copyright, NotMachineReadableError
import sys

used = set()
defined = set()


def check_license(copyright):
    for paragraph in copyright.all_files_paragraphs():
        if not paragraph.license:
            continue
        if paragraph.files:
            used.add(paragraph.license.synopsis)
    for paragraph in copyright.all_paragraphs():
        if not paragraph.license:
            continue
        if paragraph.license.text:
            defined.add(paragraph.license.synopsis)


try:
    update_copyright(check_license)
except (FileNotFoundError, NotMachineReadableError):
    pass

extra_defined = (defined - used)
extra_used = (used - defined)

if extra_used:
    sys.stderr.write('Undefined licenses in copyright: %r' % extra_used)

if extra_defined and not extra_used:
    def drop_license(copyright):
        for paragraph in list(copyright._Copyright__paragraphs):
            if paragraph.license.synopsis in extra_defined:
                copyright._Copyright__paragraphs.remove(paragraph)
    update_copyright(drop_license)

print("Remove unused license definitions for %s." % ', '.join(extra_defined))
print("Fixed-Lintian-Tags: unused-license-paragraph-in-dep5-copyright")
