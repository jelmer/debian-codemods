#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

from debian.changelog import Version
import os
import sys
import ruamel.yaml
from lintian_brush import (
    certainty_sufficient,
    min_certainty,
    )
from lintian_brush.upstream_metadata import (
    check_upstream_metadata,
    extend_upstream_metadata,
    guess_upstream_metadata_items,
    update_from_guesses,
    )
from lintian_brush.vcs import sanitize_url as sanitize_vcs_url


current_version = Version(os.environ['CURRENT_VERSION'])


if not current_version.debian_revision:
    # Native package
    sys.exit(0)


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

fields.update(update_from_guesses(
    code, current_certainty, guess_upstream_metadata_items(
        '.', trust_package=(os.environ.get('TRUST_PACKAGE') == 'true'))))

net_access = os.environ.get('NET_ACCESS', 'allow') == 'allow'
fields.update(extend_upstream_metadata(
    code, current_certainty, '.',
    minimum_certainty=minimum_certainty, net_access=net_access,
    consult_external_directory=True))
if net_access:
    # TODO(jelmer): Set package
    check_upstream_metadata(
        code, current_certainty, version=current_version.upstream_version)


# Homepage is set in debian/control, so don't add it to
# debian/upstream/metadata.
external_present_fields = set(['Homepage'])

# If the debian/copyright file is machine-readable, then we can drop the
# Name/Contact information from the debian/upstream/metadata file.
if 'Name' in code or 'Contact' in code:
    from debian.copyright import Copyright, MachineReadableFormatError
    try:
        with open('debian/copyright', 'r') as f:
            c = Copyright(f)
    except (FileNotFoundError, MachineReadableFormatError):
        pass
    else:
        if c.header.upstream_contact:
            external_present_fields.add('Contact')
        if c.header.upstream_name:
            external_present_fields.add('Name')


# Drop keys that don't need to be in debian/upsteam/metadata
for key in list(code):
    if key.startswith('X-') or key in external_present_fields:
        del code[key]
        del current_certainty[key]
        if key in fields:
            fields.remove(key)


# Drop everything that is below our minimum certainty
for key, certainty in list(current_certainty.items()):
    if not certainty_sufficient(certainty, minimum_certainty):
        del code[key]
        del current_certainty[key]
        if key in fields:
            fields.remove(key)

achieved_certainty = min_certainty(current_certainty.values())

if 'Repository' in code:
    new_repository = sanitize_vcs_url(code['Repository'])
    if new_repository != code['Repository']:
        code['Repository'] = new_repository
        fields.add('Repository')

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
