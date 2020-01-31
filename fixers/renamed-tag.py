#!/usr/bin/python3

import json
import os

from lintian_brush.lintian_overrides import (
    update_overrides,
    Override,
    )


def load_renamed_tags():
    path = os.path.abspath(os.path.join(
        os.path.dirname(__file__), '..', 'renamed-tags.json'))
    if not os.path.isfile(path):
        import pkg_resources
        path = pkg_resources.resource_filename(
            __name__, 'lintian-brush/renamed-tags.json')
        if not os.path.isfile(path):
            # Urgh.
            path = '/usr/share/lintian-brush/renamed-tags.json'
    with open(path, 'rb') as f:
        return json.load(f)


renames = load_renamed_tags()


def rename_override_tags(override):
    if override.tag in renames:
        return Override(
            override.package, override.archlist, override.type,
            renames[override.tag], override.info)
    return override


update_overrides(rename_override_tags)

print("Update renamed lintian tag names in lintian overrides.")
print("Fixed-Lintian-Tags: renamed-tag")
