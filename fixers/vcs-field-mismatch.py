#!/usr/bin/python3

from lintian_brush.control import update_control
from urllib.parse import urlparse

HOST_TO_VCS = {
    'github.com': 'Git',
    'gitlab.com': 'Git',
    'salsa.debian.org': 'Git',
    }


def fix_vcs_type(control):
    for field in list(control):
        if not field.startswith('Vcs-') or field.lower() == 'vcs-browser':
            continue
        vcs = field[4:]
        vcs_url = control[field]
        host = urlparse(vcs_url).netloc.split('@')[-1]
        if host not in HOST_TO_VCS:
            continue
        actual_vcs = HOST_TO_VCS[host]
        if actual_vcs != vcs:
            print(
                "Changed vcs type from %s to %s based on URL." % (
                    vcs, actual_vcs))
            del control["Vcs-" + vcs]
            control["Vcs-" + actual_vcs] = vcs_url


update_control(source_package_cb=fix_vcs_type)

print("Fixed-Lintian-Tags: vcs-field-mismatch")
