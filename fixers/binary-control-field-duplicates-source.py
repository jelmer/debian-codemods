#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag

removed = []

with ControlEditor() as updater:
    for binary in updater.binaries:
        for field, value in binary.items():
            if updater.source.get(field) == value:
                del binary[field]
                removed.append((binary['Package'], field, value))
                fixed_lintian_tag(
                    updater.source,
                    'binary-control-field-duplicates-source',
                    info='field "%s" in package %s' % (
                        field, binary['Package']))


report_result(
    'Remove %s that duplicate%s source.' % (
        ', '.join(
            ['%s on %s' % (field, package)
             for (package, field, value) in removed]),
        's' if len(removed) == 1 else ''))
