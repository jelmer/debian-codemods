#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    meets_minimum_certainty,
    report_result,
    )
from lintian_brush.section import (
    get_name_section_mappings,
    find_expected_section,
    )
import sys

CERTAINTY = 'likely'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)

try:
    regexes = get_name_section_mappings()
except FileNotFoundError:
    # lintian not installed?
    sys.exit(2)


default_section = None
fixed = []

with ControlEditor() as updater:
    default_section = updater.source.get("Section")

    for binary in updater.binaries:
        expected_section = find_expected_section(regexes, binary["Package"])
        section = binary.get("Section", default_section)
        if expected_section and expected_section != section:
            fixed.append(
                ('binary package %s' % binary["Package"],
                 section, expected_section))
            binary["Section"] = expected_section


# TODO(jelmer): If there is only a single binary package without section, just
# set the section of the source package?
report_result(
    "Fix sections for %s." % ', '.join(['%s (%s => %s)' % v for v in fixed]),
    certainty=CERTAINTY,
    fixed_lintian_tags=['wrong-section-according-to-package-name'])
