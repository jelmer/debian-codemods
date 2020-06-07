#!/usr/bin/python3

from debmutate.control import ControlEditor
from urllib.parse import urlparse

HOST_TO_VCS = {
    'github.com': 'Git',
    'gitlab.com': 'Git',
    'salsa.debian.org': 'Git',
    }


with ControlEditor() as updater:
    for field in list(updater.source):
        if not field.startswith('Vcs-') or field.lower() == 'vcs-browser':
            continue
        vcs = field[4:]
        vcs_url = updater.source[field]
        host = urlparse(vcs_url).netloc.split('@')[-1]
        if host not in HOST_TO_VCS:
            continue
        actual_vcs = HOST_TO_VCS[host]
        if actual_vcs != vcs:
            print(
                "Changed vcs type from %s to %s based on URL." % (
                    vcs, actual_vcs))
            del updater.source["Vcs-" + vcs]
            updater.source["Vcs-" + actual_vcs] = vcs_url


print("Fixed-Lintian-Tags: vcs-field-mismatch")
