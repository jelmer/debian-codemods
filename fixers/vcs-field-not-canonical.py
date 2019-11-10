#!/usr/bin/python3

from lintian_brush.control import update_control
from lintian_brush.vcs import canonicalize_vcs_url


fields = set()


def canonicalize_control(control):
    for name in control:
        if not name.startswith("Vcs-"):
            continue
        new_value = canonicalize_vcs_url(name[len("Vcs-"):], control[name])
        if new_value != control[name]:
            control[name] = new_value
            fields.add(name)


update_control(source_package_cb=canonicalize_control)

print("Use canonical URL in " + ', '.join(sorted(fields)) + '.')
print("Fixed-Lintian-Tags: vcs-field-not-canonical")
