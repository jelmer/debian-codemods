#!/usr/bin/python3

from lintian_brush.control import ControlUpdater
from lintian_brush.fixer import meets_minimum_certainty
import re
import sys

CERTAINTY = 'likely'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


NAME_SECTION_MAPPINGS_PATH = (
    '/usr/share/lintian/data/fields/name_section_mappings')

regexes = []

try:
    with open(NAME_SECTION_MAPPINGS_PATH, 'r') as f:
        for line in f:
            try:
                (regex, section) = line.split('=>')
            except ValueError:
                pass
            else:
                regexes.append((re.compile(regex.strip()), section.strip()))
except FileNotFoundError:
    # lintian not installed?
    sys.exit(2)


def find_expected_section(name):
    for regex, section in regexes:
        if regex.search(name):
            return section
    return None


default_section = None
fixed = []

with ControlUpdater() as updater:
    expected_section = find_expected_section(updater.source["Source"])
    if expected_section and expected_section != updater.source.get("Section"):
        fixed.append(
            ('source package', updater.source.get("Section"),
             expected_section))
        updater.source["Section"] = expected_section
    default_section = updater.source["Section"]

    for binary in updater.binaries:
        expected_section = find_expected_section(binary["Package"])
        section = binary.get("Section", default_section)
        if expected_section and expected_section != section:
            fixed.append(
                ('binary package %s' % binary["Package"],
                 section, expected_section))
            binary["Section"] = expected_section


# TODO(jelmer): Make sure that the Section is not under the Description
# TODO(jelmer): If there is only a single binary package without section, just
# set the section of the source package?
print("Fix sections for %s." % ', '.join(['%s (%s => %s)' % v for v in fixed]))
print("Certainty: %s" % CERTAINTY)
print("Fixed-Lintian-Tags: wrong-section-according-to-package-name")
