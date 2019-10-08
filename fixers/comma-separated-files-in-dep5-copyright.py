#!/usr/bin/python3

from lintian_brush.deb822 import update_deb822


def split_commas(paragraph):
    if 'Files' not in paragraph:
        return
    if ',' not in paragraph['Files']:
        return
    paragraph['Files'] = '\n' + '\n'.join(
        ' ' + entry.strip() for entry in paragraph['Files'].split(','))


try:
    update_deb822(path='debian/copyright', paragraph_cb=split_commas)
except FileNotFoundError:
    pass

print("debian/copyright: Replace commas with whitespace to separate items "
      "in Files paragraph.")
print('Fixed-Lintian-Tags: comma-separated-files-in-dep5-copyright')
