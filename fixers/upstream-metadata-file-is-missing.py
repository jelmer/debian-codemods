#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

import os
import sys
import yaml

from debian.deb822 import Deb822
with open('debian/control', 'r') as f:
    control = Deb822(f)

fields = {}
if 'XS-Go-Import-Path' in control:
    fields['Repository'] = 'https://' + control['XS-Go-Import-Path']

if not fields:
    sys.exit(0)

if not os.path.isdir('debian/upstream'):
    os.mkdir('debian/upstream')

with open('debian/upstream/metadata', 'w') as f:
    yaml.dump(fields, f, default_flow_style=False)

print('Set upstream metadata fields.')
print('Certainty: possible')
print('Fixed-Lintian-Tags: upstream-metadata-is-missing')
