#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

import os
import sys
import ruamel.yaml
from lintian_brush.upstream_metadata import (
    extend_upstream_metadata,
    guess_upstream_metadata_items,
    )


try:
    with open('debian/upstream/metadata', 'r') as f:
        inp = f.read()
except FileNotFoundError:
    code = {}
else:
    code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)

minimum_certainty = os.environ.get('MINIMUM_CERTAINTY')
fields = set()
current_certainty = {k: 'certain' for k in code.keys()}
for key, value, certainty in guess_upstream_metadata_items(
        '.', trust_package=(os.environ.get('TRUST_PACKAGE') == 'true')):
    if certainty == 'possible' and minimum_certainty == 'certain':
        continue
    if key.startswith('X-') or key in ('Name', 'Contact', 'Homepage'):
        continue
    if current_certainty.get(key) != 'certain':
        if code.get(key) != value:
            code[key] = value
            fields.add(key)
        current_certainty[key] = certainty

extend_upstream_metadata(code, current_certainty)

achieved_certainty = (
    'possible' if 'possible' in current_certainty.values() else 'certain')

if not fields:
    sys.exit(0)

if not os.path.isdir('debian/upstream'):
    os.makedirs('debian/upstream', exist_ok=True)

fixed_tag = not os.path.exists('debian/upstream/metadata')

with open('debian/upstream/metadata', 'w') as f:
    ruamel.yaml.round_trip_dump(code, f)

print('Set upstream metadata fields: %s.' % ', '.join(sorted(fields)))
print('Certainty: %s' % achieved_certainty)
if fixed_tag:
    print('Fixed-Lintian-Tags: upstream-metadata-file-is-missing')
