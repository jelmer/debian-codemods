#!/usr/bin/python3

import os

from debmutate.control import ensure_some_version, ControlEditor

from lintian_brush.fixer import report_result

needs_lsb_base = set()
other_service_present = set()

for n in os.listdir('debian'):
    if n.endswith('.init'):
        with open(os.path.join('debian', n), 'r') as f:
            for line in f:
                if line.startswith('. /lib/lsb/'):
                    needs_lsb_base.add(n.split('.')[0])
                    break
    # TODO(jelmer): This doesn't find all service files; they may not live in
    # the debian/ directory.
    if n.endswith('.service'):
        other_service_present.add(n.split('.')[0].rstrip('@'))

# TODO(jelmer): Also check that there are no implicit dependencies on lsb-base.
needs_lsb_base -= other_service_present

if needs_lsb_base:
    with ControlEditor() as updater:
        for binary in updater.binaries:
            if binary['Package'] not in needs_lsb_base:
                continue
            binary['Depends'] = ensure_some_version(
                binary['Depends'], 'lsb-base')

report_result(
    'Add missing dependency on lsb-base.',
    fixed_lintian_tags=['init.d-script-needs-depends-on-lsb-base'],
    certainty='possible')
