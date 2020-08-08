#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    fixed_lintian_tag,
    report_result,
    )
from lintian_brush.section import (
    find_expected_section,
    get_name_section_mappings,
    )
import sys

binary_sections_set = set()
source_section_set = False
regexes = None

with ControlEditor() as updater:
    if updater.source.get('Section'):
        sys.exit(0)
    binary_sections = set()
    for binary in updater.binaries:
        if not binary.get('Section'):
            if regexes is None:
                regexes = get_name_section_mappings()
            section = find_expected_section(regexes, binary['Package'])
            if section:
                binary['Section'] = section
                binary_sections_set.add(binary['Package'])
                fixed_lintian_tag(binary, 'recommended-field', 'Section')
        if binary.get('Section'):
            binary_sections.add(binary['Section'])
    if len(binary_sections) == 1:
        updater.source['Section'] = binary_sections.pop()
        for binary in updater.binaries:
            try:
                del binary['Section']
            except KeyError:
                pass
        fixed_lintian_tag(updater.source, 'recommended-field', 'Section')
        source_section_set = True
    if source_section_set and binary_sections_set:
        report_result(
            'Section field set in source based on binary package names.',
            certainty='certain')
    elif source_section_set:
        report_result(
            'Section field set in source stanza rather than binary packages.',
            certainty='certain')
    elif binary_sections_set:
        report_result(
            'Section field set for binary packages %s based on name.'
            % ', '.join(sorted(binary_sections_set)),
            certainty='certain')
