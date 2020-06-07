#!/usr/bin/python3

from debian.copyright import (
    LicenseParagraph,
    NotMachineReadableError,
    )
from debmutate.copyright import CopyrightEditor
from lintian_brush.fixer import report_result


def fix_header_license_references(copyright):
    if not copyright.header.license:
        return
    if not copyright.header.license.text:
        return
    used_licenses = set()
    seen_licenses = set()
    for files_paragraph in copyright.all_files_paragraphs():
        if not files_paragraph.license:
            continue
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
    return copyright.header.license


try:
    with CopyrightEditor() as updater:
        license = fix_header_license_references(updater.copyright)
except (FileNotFoundError, NotMachineReadableError):
    pass
else:
    if license:
        report_result(
            'Add missing license paragraph for %s' % license.synopsis,
            fixed_lintian_tags=[
                'dep5-file-paragraph-references-header-paragraph'])
