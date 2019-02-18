#!/usr/bin/python3

from debian.copyright import Copyright
import sys

header = None
files = {}

with open('debian/copyright', 'r') as f:
    content = f.read()

copyright = Copyright(content)
if copyright.dump() != content:
    sys.exit(2)

files_i = 0
for i, paragraph in enumerate(copyright._Copyright__paragraphs):
    if "Files" in paragraph:
        if paragraph["Files"] == "*" and files_i > 0:
            copyright._Copyright__paragraphs.insert(
                0, copyright._Copyright__paragraphs.pop(i))
        files_i += 1


with open('debian/copyright', 'w') as f:
    copyright.dump(f)

print('Make "Files: *" paragraph the first in the copyright file.')
print('Fixed-Lintian-Tags: '
      'global-files-wildcard-not-first-paragraph-in-dep5-copyright')
