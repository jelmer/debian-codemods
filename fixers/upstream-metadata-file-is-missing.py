#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

import os
import sys
import ruamel.yaml
from lintian_brush.upstream_metadata import guess_upstream_metadata_items


class MetadataUnpreservable(Exception):
    """Unable to preserve formatting of debian/upstream/metadata."""


try:
    with open('debian/upstream/metadata', 'r') as f:
        inp = f.read()
except FileNotFoundError:
    code = {}
else:
    code = ruamel.yaml.load(inp, ruamel.yaml.RoundTripLoader)
    roundtrip_inp = ruamel.yaml.dump(code, Dumper=ruamel.yaml.RoundTripDumper)
    if roundtrip_inp != inp:
        raise MetadataUnpreservable()

minimum_certainty = os.environ.get('MINIMUM_CERTAINTY')
fields = set()
current_certainty = {k: 'certain' for k in code.keys()}
for key, value, certainty in guess_upstream_metadata_items(
        '.', trust_package=(os.environ.get('TRUST_PACKAGE') == 'true')):
    if certainty == 'possible' and minimum_certainty == 'certain':
        continue
    if current_certainty.get(key) != 'certain':
        code[key] = value
        current_certainty[key] = certainty
        fields.add(key)

achieved_certainty = (
    'possible' if 'possible' in current_certainty.values() else 'certain')

if not code:
    sys.exit(0)

if not os.path.isdir('debian/upstream'):
    os.makedirs('debian/upstream', exist_ok=True)

fixed_tag = not os.path.exists('debian/upstream/metadata')

with open('debian/upstream/metadata', 'w') as f:
    ruamel.yaml.dump(code, f, Dumper=ruamel.yaml.RoundTripDumper)

print('Set upstream metadata fields: %s.' % ', '.join(sorted(fields)))
print('Certainty: %s' % achieved_certainty)
if fixed_tag:
    print('Fixed-Lintian-Tags: upstream-metadata-file-is-missing')
