#!/usr/bin/python3

from debian.copyright import (
    LicenseParagraph,
    NotMachineReadableError,
    )
from lintian_brush.copyright import update_copyright

license = None


def fix_header_license_references(copyright):
    global license

    if not copyright.header.license:
        return
    if not copyright.header.license.text:
        return
    used_licenses = set()
    seen_licenses = set()
    for files_paragraph in copyright.all_files_paragraphs():
        used_licenses.add(files_paragraph.license.synopsis)
        if files_paragraph.license.text:
            seen_licenses.add(files_paragraph.license.synopsis)
    for license_paragraph in copyright.all_license_paragraphs():
        seen_licenses.add(license_paragraph.license.synopsis)
    for missing in used_licenses - seen_licenses:
        if copyright.header.license.synopsis == missing:
            copyright.add_license_paragraph(
                LicenseParagraph.create(
                    copyright.header.license))
    license = copyright.header.license


try:
    update_copyright(fix_header_license_references)
except (FileNotFoundError, NotMachineReadableError):
    pass
else:
    if license:
        print('Add missing license paragraph for %s' % license.synopsis)
        print(
            'Fixed-Lintian-Tags: '
            'dep5-file-paragraph-references-header-paragraph')
