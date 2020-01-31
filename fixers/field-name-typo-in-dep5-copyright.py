#!/usr/bin/python3

from lintian_brush.deb822 import Deb822Updater
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

try:
    with Deb822Updater('debian/copyright') as updater:
        for paragraph in updater.paragraphs:
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
except FileNotFoundError:
    pass


print('Fix field name typos in debian/copyright.')
print('Fixed-Lintian-Tags: field-name-typo-dep5-copyright')
