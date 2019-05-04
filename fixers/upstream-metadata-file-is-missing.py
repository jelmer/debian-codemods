#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

import os
import sys
import ruamel.yaml
import subprocess
import tempfile

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

    if os.path.exists('setup.py'):
        with tempfile.TemporaryDirectory() as td:
            subprocess.call(
                ['python', os.path.abspath('setup.py'), 'dist_info'], cwd=td,
                stderr=subprocess.PIPE, stdout=subprocess.PIPE)
            [name] = os.listdir(td)
            with open(os.path.join(td, name, 'PKG-INFO'), 'r') as f:
                python_info = [
                    l.rstrip('\n').split(': ', 1) for l in f.readlines()]
        for key, value in python_info:
            if key == 'Name':
                code['Name'] = value
            if key == 'Project-URL':
                url_type, url = value.split(', ')
                if url_type in ('GitHub', 'Repository'):
                    code['Repository'] = url


if not code:
    sys.exit(0)

if not os.path.isdir('debian/upstream'):
    os.mkdir('debian/upstream')

with open('debian/upstream/metadata', 'w') as f:
    ruamel.yaml.dump(code, f, Dumper=ruamel.yaml.RoundTripDumper)

print('Set upstream metadata fields.')
print('Certainty: possible')
print('Fixed-Lintian-Tags: upstream-metadata-is-missing')
