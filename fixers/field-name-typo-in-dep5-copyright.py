#!/usr/bin/python3

from lintian_brush.deb822 import update_deb822
import sys

try:
    from Levenshtein import distance
except ImportError:
    sys.exit(2)

valid_field_names = {
    'Files', 'License', 'Copyright', 'Comment',
    'Upstream-Name', 'Format', 'Upstream-Contact',
    'Source', 'Upstream', 'Contact', 'Name'}

fixed = False


def fix_field_typos(paragraph):
    global fixed
    for field in paragraph:
        if field in valid_field_names:
            continue
        for option in valid_field_names:
            if distance(field, option) == 1:
                value = paragraph[field]
                del paragraph[field]
                paragraph[option] = value
                fixed = True
                break


try:
    update_deb822('debian/copyright', paragraph_cb=fix_field_typos)
except FileNotFoundError:
    pass


print('Fix field name typos in debian/copyright.')
print('Fixed-Lintian-Tags: field-name-typo-dep5-copyright')
