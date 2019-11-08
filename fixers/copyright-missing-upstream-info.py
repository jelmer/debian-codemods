#!/usr/bin/python3

import os

from lintian_brush import certainty_to_confidence, certainty_sufficient
from lintian_brush.copyright import update_copyright, NotMachineReadableError
from lintian_brush.upstream_metadata import guess_upstream_metadata_items


minimum_certainty = os.environ.get('MINIMUM_CERTAINTY')
fields = []
achieved_certainty = []


def add_upstream_metadata(copyright):
    if copyright.header.upstream_name and copyright.header.upstream_contact:
        return
    import ruamel.yaml
    try:
        with open('debian/upstream/metadata', 'r') as f:
            inp = f.read()
    except FileNotFoundError:
        upstream_metadata = {}
    else:
        code = ruamel.yaml.safe_load(inp)
        upstream_metadata = {k: (v, 'certain') for (k, v) in code.items()}
    for key, value, certainty in guess_upstream_metadata_items(
            '.', trust_package=(os.environ.get('TRUST_PACKAGE') == 'true')):
        if not certainty_sufficient(certainty, minimum_certainty):
            continue
        if upstream_metadata.get(key, (None, None))[1] != 'certain':
            upstream_metadata[key] = (value, certainty)

    if not copyright.header.upstream_name:
        try:
            (value, certainty) = upstream_metadata['Name']
        except KeyError:
            pass
        else:
            if value:
                copyright.header.upstream_name = value
                fields.append('Upstream-Name')
                achieved_certainty.append(certainty)

    if not copyright.header.upstream_contact:
        try:
            (value, certainty) = upstream_metadata['Contact']
        except KeyError:
            pass
        else:
            if value:
                if isinstance(value, str):
                    value = [value]
                copyright.header.upstream_contact = value
                fields.append('Upstream-Contact')
                achieved_certainty.append(certainty)


try:
    update_copyright(add_upstream_metadata)
except (FileNotFoundError, NotMachineReadableError):
    pass

print('Set fields %s in debian/copyright.' % ', '.join(fields))
if achieved_certainty:
    print('Certainty: %s' %
          min(achieved_certainty, key=certainty_to_confidence))
