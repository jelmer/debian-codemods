#!/usr/bin/python3

from lintian_brush.control import ControlUpdater
from lintian_brush.fixer import report_result
from lintian_brush.vcs import canonicalize_vcs_url


fields = set()


with ControlUpdater() as updater:
    for name in updater.source:
        if not name.startswith("Vcs-"):
            continue
        new_value = canonicalize_vcs_url(
            name[len("Vcs-"):], updater.source[name])
        if new_value != updater.source[name]:
            updater.source[name] = new_value
            fields.add(name)

report_result(
    "Use canonical URL in " + ', '.join(sorted(fields)) + '.',
    fixed_lintian_tags=['vcs-field-not-canonical'])
