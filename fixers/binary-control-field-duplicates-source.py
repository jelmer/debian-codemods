#!/usr/bin/python3

import sys
from typing import Dict, Set

from lintian_brush.fixer import control, report_result, LintianIssue

removed: Dict[str, Set[str]] = {}

try:
    with control as updater:
        for binary in updater.binaries:
            for field, value in list(binary.items()):
                if updater.source.get(field) == value:
                    del binary[field]
                    removed.setdefault(field, set()).add(binary['Package'])
                    issue = LintianIssue(
                        updater.source,
                        'installable-field-mirrors-source',
                        info='field "%s" in package %s' % (
                            field, binary['Package']))
                    issue.report_fixed()
except FileNotFoundError:
    sys.exit(0)


if len(removed) == 1:
    (field, binary_packages) = list(removed.items())[0]
    report_result(
        'Remove field %s on binary package%s %s that duplicates source.' % (
            field, 's' if len(binary_packages) != 1 else '',
            ', '.join(sorted(binary_packages))))
elif len(removed) > 1:
    report_result(
            'Remove fields on binary packages that duplicate source:\n'
            + '\n'.join(
                ['+ Field %s from %s.' % (
                    field, ', '.join(sorted(packages)))
                    for (field, packages) in sorted(removed.items())]))
