#!/usr/bin/python3

from contextlib import suppress

from debmutate.copyright import CopyrightEditor, NotMachineReadableError
from upstream_ontologist import (
    UpstreamDatum,
)
from upstream_ontologist.guess import (
    guess_upstream_metadata_items,
)

from lintian_brush import min_certainty
from lintian_brush.fixer import (
    meets_minimum_certainty,
    report_result,
    trust_package,
)

fields = []
achieved_certainty = []


def add_upstream_metadata(copyright):
    if copyright.header.upstream_name and copyright.header.upstream_contact:
        return
    import ruamel.yaml
    try:
        with open('debian/upstream/metadata') as f:
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


with suppress(FileNotFoundError, NotMachineReadableError), \
        CopyrightEditor() as updater:
    add_upstream_metadata(updater.copyright)

certainty = min_certainty(achieved_certainty)

if len(fields) == 1:
    report_result('Set field %s in debian/copyright.' % ', '.join(fields),
                  certainty=certainty)
else:
    report_result('Set fields %s in debian/copyright.' % ', '.join(fields),
                  certainty=certainty)
