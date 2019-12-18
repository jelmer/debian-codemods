#!/usr/bin/python3

import sys

from lintian_brush.lintian_overrides import (
    update_overrides,
    Override,
    )

renames = {}
try:
    with open('/usr/share/lintian/data/override/renamed-tags', 'r') as f:
        for line in f:
            if line.startswith('#'):
                continue
            if not line.strip():
                continue
            (old, new) = line.split('=>')
            renames[old.strip()] = new.strip()
except FileNotFoundError:
    sys.exit(2)


def rename_override_tags(override):
    if override.tag in renames:
        return Override(
            override.package, override.archlist, override.type,
            renames[override.tag], override.info)
    return override


update_overrides(rename_override_tags)

print("Update renamed lintian tag names in lintian overrides.")
print("Fixed-Lintian-Tags: renamed-tag")
