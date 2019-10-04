#!/usr/bin/python3

from lintian_brush.lintian_overrides import (
    update_overrides,
    Override,
    )


renames = {
    'debian-watch-may-check-gpg-signature':
        'debian-watch-does-not-check-gpg-signature',
    'systemd-no-service-for-init-script':
        'omitted-systemd-service-for-init.d-script',
    'rules-requires-root-implicitly': 'rules-requires-root-missing',
}


def rename_override_tags(override):
    if override.tag in renames:
        return Override(
            override.package, override.archlist, override.type,
            renames[override.tag], override.info)
    return override


update_overrides(rename_override_tags)

print("Update renamed lintian tag names in lintian overrides.")
print("Fixed-Lintian-Tags: renamed-tag")
