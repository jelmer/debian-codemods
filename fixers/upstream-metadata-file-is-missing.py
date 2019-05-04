#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

import os
import sys
import ruamel.yaml

from debian.deb822 import Deb822
with open('debian/control', 'r') as f:
    control = Deb822(f)

try:
    with open('debian/upstream/metadata', 'r') as f:
        inp = f.read()
except FileNotFoundError:
    code = {}
else:
    code = ruamel.yaml.load(inp, ruamel.yaml.RoundTripLoader)


if 'Repository' not in code:
    if 'XS-Go-Import-Path' in control:
        code['Repository'] = 'https://' + control['XS-Go-Import-Path']

if not code:
    sys.exit(0)

if not os.path.isdir('debian/upstream'):
    os.mkdir('debian/upstream')

with open('debian/upstream/metadata', 'w') as f:
    ruamel.yaml.dump(code, f, Dumper=ruamel.yaml.RoundTripDumper)

print('Set upstream metadata fields.')
print('Certainty: possible')
print('Fixed-Lintian-Tags: upstream-metadata-is-missing')
