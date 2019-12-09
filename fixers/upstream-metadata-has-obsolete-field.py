#!/usr/bin/python3

from lintian_brush.upstream_metadata import ADDON_ONLY_FIELDS
from lintian_brush.yaml import YamlUpdater

obsolete_fields = set()
removed_fields = []

with YamlUpdater('debian/upstream/metadata') as code:

    # If the debian/copyright file is machine-readable, then we can drop the
    # Name/Contact information from the debian/upstream/metadata file.
    if 'Name' in code or 'Contact' in code:
        from lintian_brush.copyright import upstream_fields_in_copyright
        obsolete_fields.update(
            upstream_fields_in_copyright('debian/copyright'))

    for field in obsolete_fields:
        if field in code:
            del code[field]
            removed_fields.append(field)

    if removed_fields:
        if not (set(code.keys()) - set(ADDON_ONLY_FIELDS)):
            code.clear()


print('Remove obsolete field%s %s from debian/upstream/metadata '
      '(already present in machine-readable debian/copyright).' %
      ('s' if len(removed_fields) > 1 else '',
       ', '.join(sorted(removed_fields))))
