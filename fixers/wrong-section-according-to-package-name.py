#!/usr/bin/python3

from lintian_brush.control import update_control
import re
import sys

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


def fix_source_section(control):
    global default_section
    expected_section = find_expected_section(control["Source"])
    if expected_section and expected_section != control.get("Section"):
        fixed.append(
            ('source package', control.get("Section"), expected_section))
        control["Section"] = expected_section
    default_section = control["Section"]


def fix_binary_section(control):
    expected_section = find_expected_section(control["Package"])
    section = control.get("Section", default_section)
    if expected_section and expected_section != section:
        fixed.append(
            ('binary package %s' % control["Package"],
             section, expected_section))
        control["Section"] = expected_section


update_control(
    source_package_cb=fix_source_section, binary_package_cb=fix_binary_section)

print("Fix sections for %s." % ', '.join(['%s (%s => %s)' % v for v in fixed]))
print("Fixed-Lintian-Tags: wrong-section-according-to-package-name")
