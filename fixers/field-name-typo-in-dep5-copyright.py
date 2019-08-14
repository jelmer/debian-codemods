#!/usr/bin/python3

from debian.deb822 import Deb822
from lintian_brush.deb822 import reformat_deb822
from lintian_brush.reformatting import check_preserve_formatting
import sys

try:
    from Levenshtein import distance
except ImportError:
    sys.exit(2)

with open('debian/copyright', 'rb') as f:
    orig_content = f.read()

rewrite_content = reformat_deb822(orig_content)
check_preserve_formatting(
    rewrite_content.strip(), orig_content.strip(),
    'debian/copyright')

valid_field_names = {
    'Files', 'License', 'Copyright', 'Comment',
    'Upstream-Name', 'Format', 'Upstream-Contact',
    'Source', 'Upstream', 'Contact', 'Name'}

fixed = False
paragraphs = list(Deb822.iter_paragraphs(orig_content))
for paragraph in paragraphs:
    for field in paragraph:
        if field in valid_field_names:
            continue
        for option in valid_field_names:
            if distance(field, option) == 1:
                paragraph[option] = paragraph[field]
                del paragraph[field]
                fixed = True
                break

if not fixed:
    sys.exit(2)

with open('debian/copyright', 'wb') as f:
    for paragraph in paragraphs:
        paragraph.dump(fd=f)
        f.write(b'\n')

print('Fix field name typos in debian/copyright.')
print('Fixed-Lintian-Tags: field-name-typo-dep5-copyright')
