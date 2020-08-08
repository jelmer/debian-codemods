#!/usr/bin/python3

import sys

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    fixed_lintian_tag,
    meets_minimum_certainty,
    net_access_allowed,
    report_result,
    trust_package,
    )
from lintian_brush.upstream_metadata import guess_upstream_metadata

CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


def guess_description(binary_name, all_binaries):
    if len(all_binaries) != 1:
        # TODO(jelmer): Support handling multiple binaries
        return None
    upstream_metadata = guess_upstream_metadata(
        '.', trust_package(), net_access_allowed())
    try:
        description = upstream_metadata['X-Summary']
    except KeyError:
        return None
    try:
        lines = upstream_metadata['X-Description'].splitlines()
    except KeyError:
        return description
    lines = [line if line else '.' for line in lines]
    description += "\n" + ''.join([" %s\n" % line for line in lines])
    return description.rstrip('\n')


updated = []

with ControlEditor() as updater:
    for binary in updater.binaries:
        if binary.get('Description'):
            continue
        description = guess_description(binary['Package'], updater.binaries)
        if description:
            binary['Description'] = description
            updated.append(binary['Package'])
            fixed_lintian_tag(binary, 'required-field', 'Description')


report_result(
    description='Add description for binary packages: %s' %
    ', '.join(sorted(updated)),
    certainty=CERTAINTY)
