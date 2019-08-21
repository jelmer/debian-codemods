#!/usr/bin/python3

import os
import ruamel.yaml

OBSOLETE_FIELDS = ['Name', 'Contact']
removed_fields = []

try:
    with open('debian/upstream/metadata', 'r') as f:
        inp = f.read()
except FileNotFoundError:
    code = {}
else:
    code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)

for field in OBSOLETE_FIELDS:
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
      ', '.join(removed_fields))
