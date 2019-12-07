#!/usr/bin/python3

import os

from lintian_brush.control import update_control
from lintian_brush.vcs import find_secure_vcs_url

updated = set()
net_access = os.environ.get('NET_ACCESS', 'disallow') == 'allow'
lp_note = False


def fix_insecure_vcs_uri(control):
    global updated, lp_note
    for key, value in control.items():
        if not key.startswith('Vcs-'):
            continue
        if value.startswith('lp:'):
            lp_note = True
        newvalue = find_secure_vcs_url(value, net_access=net_access)
        if newvalue is None:
            # We can't find a secure version :(
            continue
        if newvalue == value:
            # The URL was already secure
            continue
        control[key] = newvalue
        updated.add(key)


update_control(source_package_cb=fix_insecure_vcs_uri)

if len(updated) == 1:
    print("Use secure URI in Vcs control header %s." % list(updated)[0])
else:
    print("Use secure URI in Vcs control headers: %s." %
          ', '.join(sorted(updated)))
if lp_note:
    print("")
    print("The lp: prefix gets expanded to http://code.launchpad.net/ for "
          "users that are not logged in on some versions of Bazaar.")
print("Fixed-Lintian-Tags: vcs-field-uses-insecure-uri")
