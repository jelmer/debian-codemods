#!/usr/bin/python3

import os
import sys

from typing import List

from debmutate.control import (
    ensure_exact_version,
    iter_relations,
    is_relation_implied,
    ControlEditor,
    parse_relations,
    format_relations,
    )
from debmutate.debhelper import (
    read_debhelper_compat_file,
    )
from lintian_brush.fixer import report_result
from lintian_brush.rules import (
    check_cdbs,
    )


if not os.path.exists('debian/compat'):
    sys.exit(0)

# Package currently stores compat version in debian/compat..

debhelper_compat_version = read_debhelper_compat_file('debian/compat')

# debhelper >= 11 supports the magic debhelper-compat build-dependency.
# Exclude cdbs, since it only knows to get the debhelper compat version
# from debian/compat.

if debhelper_compat_version < 11 or check_cdbs():
    sys.exit(0)

# Upgrade to using debhelper-compat, drop debian/compat file.
os.unlink('debian/compat')

# Assume that the compat version is set in Build-Depends
with ControlEditor() as updater:
    insert_position = None
    changed_fields = []
    for field in ['Build-Depends', 'Build-Depends-Indep',
                  'Build-Depends-Arch']:
        to_delete: List[int] = []
        for offset, relation in iter_relations(
                updater.source.get(field, ''), 'debhelper'):
            if (field == 'Build-Depends' and
                    set([r.name for r in relation]) == set(['debhelper'])):
                # In the simple case, we'd just replace the debhelper
                # dependency with a debhelper-compat one, so remember the
                # location.
                insert_position = offset - len(to_delete)
            if is_relation_implied(
                    relation, 'debhelper (>= %d)' % debhelper_compat_version):
                to_delete.append(offset)

        if to_delete:
            # TODO(jelmer): Move this into a helper function in
            # lintian_brush.control.
            relations = parse_relations(updater.source[field])
            for i in reversed(to_delete):
                if i == 0 and len(relations) > 1:
                    # If the first item is removed, then copy the spacing to
                    # the next item
                    relations[1] = (
                        relations[0][0], relations[1][1], relations[0][2])
                del relations[i]

            updater.source[field] = format_relations(relations)
            changed_fields.append(field)

    updater.source["Build-Depends"] = ensure_exact_version(
        updater.source.get("Build-Depends", ""), "debhelper-compat",
        "%d" % debhelper_compat_version, position=insert_position)

    for field in changed_fields:
        if updater.source.get(field) == "":
            del updater.source[field]

report_result(
    "Set debhelper-compat version in Build-Depends.",
    fixed_lintian_tags=['uses-debhelper-compat-file'])
