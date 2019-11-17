#!/usr/bin/python3

import os
import ruamel.yaml

obsolete_fields = set()
removed_fields = []

try:
    with open('debian/upstream/metadata', 'r') as f:
        inp = f.read()
except FileNotFoundError:
    code = {}
else:
    code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)

# If the debian/copyright file is machine-readable, then we can drop the
# Name/Contact information from the debian/upstream/metadata file.
if 'Name' in code or 'Contact' in code:
    from lintian_brush.copyright import upstream_fields_in_copyright
    obsolete_fields.update(upstream_fields_in_copyright('debian/copyright'))


for field in obsolete_fields:
    if field in code:
        del code[field]
        removed_fields.append(field)

if removed_fields:
    if code:
        with open('debian/upstream/metadata', 'w') as f:
            ruamel.yaml.round_trip_dump(code, f)
    else:
        os.unlink('debian/upstream/metadata')
        if os.listdir('debian/upstream') == []:
            os.rmdir('debian/upstream')

print('Remove obsolete fields %s from debian/upstream/metadata.' %
      ', '.join(sorted(removed_fields)))
