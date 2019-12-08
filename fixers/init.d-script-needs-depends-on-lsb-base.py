#!/usr/bin/python3

from lintian_brush.control import update_control, ensure_some_version
import os

needs_lsb_base = set()
other_service_present = set()

for n in os.listdir('debian'):
    if n.endswith('.init'):
        with open(os.path.join('debian', n), 'r') as f:
            for l in f:
                if l.startswith('. /lib/lsb/'):
                    needs_lsb_base.add(n.split('.')[0])
                    break
    # TODO(jelmer): This doesn't find all service files; they may not live in
    # the debian/ directory.
    if n.endswith('.service'):
        other_service_present.add(n.split('.')[0].rstrip('@'))

# TODO(jelmer): Also check that there are no implicit dependencies on lsb-base.
needs_lsb_base -= other_service_present

if needs_lsb_base:
    def add_lsb_base(binary):
        if binary['Package'] not in needs_lsb_base:
            return
        binary['Depends'] = ensure_some_version(binary['Depends'], 'lsb-base')
    update_control(binary_package_cb=add_lsb_base)

print('Add missing dependency on lsb-base.')
print('Fixed-Lintian-Tags: init.d-script-needs-depends-on-lsb-base')
print('Certainty: possible')
