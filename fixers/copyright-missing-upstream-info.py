#!/usr/bin/python3

from lintian_brush import certainty_to_confidence
from debmutate.copyright import CopyrightEditor, NotMachineReadableError
from lintian_brush.fixer import (
    meets_minimum_certainty,
    trust_package,
    )
from lintian_brush.upstream_metadata import (
    guess_upstream_metadata_items,
    UpstreamDatum,
    )


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
        upstream_metadata = {
            k: UpstreamDatum(k, v, 'certain') for (k, v) in code.items()}
    for datum in guess_upstream_metadata_items(
            '.', trust_package=trust_package()):
        if not meets_minimum_certainty(datum.certainty):
            continue
        if (datum.field not in upstream_metadata or
                upstream_metadata[datum.field].certainty != 'certain'):
            upstream_metadata[datum.field] = datum

    if not copyright.header.upstream_name:
        try:
            datum = upstream_metadata['Name']
        except KeyError:
            pass
        else:
            if datum.value:
                copyright.header.upstream_name = datum.value
                fields.append('Upstream-Name')
                achieved_certainty.append(datum.certainty)

    if not copyright.header.upstream_contact:
        try:
            datum = upstream_metadata['Contact']
        except KeyError:
            pass
        else:
            if datum.value:
                value = datum.value
                if isinstance(value, str):
                    value = [value]
                copyright.header.upstream_contact = value
                fields.append('Upstream-Contact')
                achieved_certainty.append(datum.certainty)


try:
    with CopyrightEditor() as updater:
        add_upstream_metadata(updater.copyright)
except (FileNotFoundError, NotMachineReadableError):
    pass

if len(fields) == 1:
    print('Set field %s in debian/copyright.' % ', '.join(fields))
else:
    print('Set fields %s in debian/copyright.' % ', '.join(fields))
if achieved_certainty:
    print('Certainty: %s' %
          min(achieved_certainty, key=certainty_to_confidence))
