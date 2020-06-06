#!/usr/bin/python3

from debian.copyright import (
    Copyright,
    FilesParagraph,
    LicenseParagraph,
    License,
    )
from lintian_brush.fixer import report_result, meets_minimum_certainty

import os
import sys

CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)

if os.path.exists('debian/copyright'):
    sys.exit(0)


try:
    import decopy  # noqa: F401
except ModuleNotFoundError:
    # No decopy
    sys.exit(2)

from decopy.tree import RootInfo, DirInfo  # noqa: E402
from decopy.datatypes import License as DecopyLicense  # noqa: E402
from decopy.dep5 import Copyright as DecopyCopyright, Group  # noqa: E402
from decopy.cmdoptions import process_options  # noqa: E402

options = process_options([
    '--root=.',
    '--no-progress',
    '--mode=full',
    '--output=debian/copyright',
    ])

filetree = RootInfo.build(options)
copyright_ = DecopyCopyright.build(filetree, options)

copyright_.process(filetree)
filetree.process(options)

groups = copyright_.get_group_dict(options)

for fileinfo in filetree:
    if fileinfo.group:
        continue
    if isinstance(fileinfo, DirInfo):
        continue

    file_key = fileinfo.get_group_key(options)
    group = groups.setdefault(file_key, Group(file_key))
    group.add_file(fileinfo)
    fileinfo.group = group

licenses = set()

c = Copyright()
# Print files paragraphs
for _, group in sorted(
        groups.items(), key=lambda i: i[1].sort_key(options)):

    if not group.copyright_block_valid():
        continue

    licenses.update(group.licenses.keys())

    if options.glob:
        files = group.files.get_patterns()
    else:
        files = group.files.sorted_members()

    if group.copyrights:
        holders = '\n           '.join(group.copyrights.sorted_members())
    else:
        holders = 'Unknown'
    paragraph = FilesParagraph.create(
        list(files), holders, License(group.license))

    comments = group.get_comments()
    if comments:
        paragraph.comment = comments

    c.add_files_paragraph(paragraph)

# Print license paragraphs
for key in sorted(licenses):
    license_ = DecopyLicense.get(key)
    paragraph = LicenseParagraph.create(
        License(license_.name))
    paragraph.comment = "Add the corresponding license text here"
    c.add_license_paragraph(paragraph)


with open('debian/copyright', 'w') as f:
    c.dump(f)


report_result(
    'Create a debian/copyright file.',
    certainty=CERTAINTY,
    fixed_lintian_tags=['no-copyright-file'])
