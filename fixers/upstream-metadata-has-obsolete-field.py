#!/usr/bin/python3

import re

from lintian_brush.upstream_metadata import ADDON_ONLY_FIELDS
from lintian_brush.yaml import YamlUpdater

SEP_CHARS = r'\n+|\s\s+|\t+'

obsolete_fields = {}
removed_fields = []

with YamlUpdater('debian/upstream/metadata') as editor:

    # If the debian/copyright file is machine-readable, then we can drop the
    # Name/Contact information from the debian/upstream/metadata file.
    if 'Name' in editor.code or 'Contact' in editor.code:
        from debmutate.copyright import upstream_fields_in_copyright
        obsolete_fields.update(
            upstream_fields_in_copyright('debian/copyright'))

    for field, copyright_value in obsolete_fields.items():
        try:
            um_value = editor.code[field]
        except KeyError:
            continue
        if isinstance(copyright_value, tuple):
            copyright_entries = [x.strip() for x in copyright_value]
        else:
            copyright_entries = re.split(SEP_CHARS, copyright_value)
        um_entries = re.split(SEP_CHARS, um_value)
        um_entries = [x.lower() for x in um_entries]
        copyright_entries = [x.lower() for x in copyright_entries]
        if set(um_entries) == set(copyright_entries):
            del editor.code[field]
            removed_fields.append(field)

    if removed_fields:
        if not (set(editor.code.keys()) - set(ADDON_ONLY_FIELDS)):
            editor.code.clear()


print('Remove obsolete field%s %s from debian/upstream/metadata '
      '(already present in machine-readable debian/copyright).' %
      ('s' if len(removed_fields) > 1 else '',
       ', '.join(sorted(removed_fields))))
