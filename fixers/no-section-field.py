#!/usr/bin/python3

import sys
from contextlib import suppress

from lintian_brush.fixer import (
    LintianIssue,
    control,
    report_result,
)
from lintian_brush.section import (
    find_expected_section,
    get_name_section_mappings,
)

binary_sections_set = set()
source_section_set = False
regexes = None

with control as updater:
    if updater.source.get("Section"):
        sys.exit(0)
    binary_sections = set()
    for binary in updater.binaries:
        if not binary.get("Section"):
            if regexes is None:
                regexes = get_name_section_mappings()
            section = find_expected_section(regexes, binary["Package"])
            issue = LintianIssue(binary, "recommended-field", "Section")
            if section and issue.should_fix():
                binary["Section"] = section
                binary_sections_set.add(binary["Package"])
                issue.report_fixed()
        if binary.get("Section"):
            binary_sections.add(binary["Section"])
    issue = LintianIssue(updater.source, "recommended-field", "Section")
    if len(binary_sections) == 1 and issue.should_fix():
        updater.source["Section"] = binary_sections.pop()
        for binary in updater.binaries:
            with suppress(KeyError):
                del binary["Section"]
        issue.report_fixed()
        source_section_set = True
    if source_section_set and binary_sections_set:
        report_result(
            "Section field set in source based on binary package names.",
            certainty="certain",
        )
    elif source_section_set:
        report_result(
            "Section field set in source stanza rather than binary packages.",
            certainty="certain",
        )
    elif binary_sections_set:
        report_result(
            "Section field set for binary packages {} based on name.".format(", ".join(sorted(binary_sections_set))),
            certainty="certain",
        )
