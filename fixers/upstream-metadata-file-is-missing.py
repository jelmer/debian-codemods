#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

import os
import sys
import ruamel.yaml
from lintian_brush.upstream_metadata import guess_upstream_metadata

try:
    with open('debian/upstream/metadata', 'r') as f:
        inp = f.read()
except FileNotFoundError:
    code = {}
else:
    code = ruamel.yaml.load(inp, ruamel.yaml.RoundTripLoader)

fields = set()
guessed_metadata = guess_upstream_metadata(
    '.', trust=('TRUST_PACKAGE' in os.environ))
for key, value in guessed_metadata.items():
    if key not in code:
        code[key] = value
        fields.add(key)

if not code:
    sys.exit(0)

if not os.path.isdir('debian/upstream'):
    os.mkdir('debian/upstream')

with open('debian/upstream/metadata', 'w') as f:
    ruamel.yaml.dump(code, f, Dumper=ruamel.yaml.RoundTripDumper)

print('Set upstream metadata fields: %s.' % ', '.join(sorted(fields)))
print('Certainty: possible')
print('Fixed-Lintian-Tags: upstream-metadata-is-missing')
