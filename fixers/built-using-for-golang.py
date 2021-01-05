#!/usr/bin/python3

import sys

from debmutate.control import (
    add_dependency,
    drop_dependency,
    get_relation,
    iter_relations,
    )
from lintian_brush.fixer import control, report_result, LintianIssue

added = []
removed = []
go_package = False
default_architecture = None


with control as updater:
    if any(iter_relations(updater.source.get('Build-Depends', ''),
                          'golang-go')):
        go_package = True
    if any(iter_relations(updater.source.get('Build-Depends', ''),
                          'golang-any')):
        go_package = True
    if not go_package:
        sys.exit(0)

    default_architecture = updater.source.get('Architecture')

    for binary in updater.binaries:
        if binary.get('Architecture', default_architecture) == 'all':
            if 'Built-Using' in binary:
                issue = LintianIssue(
                    updater.source,
                    'built-using-field-on-arch-all-package',
                    binary['Package'])
                if issue.should_fix():
                    binary['Built-Using'] = drop_dependency(
                        binary['Built-Using'], '${misc:Built-Using}')
                    if not binary['Built-Using']:
                        del binary['Built-Using']
                    removed.append(binary['Package'])
                    issue.report_fixed()
        else:
            built_using = binary.get('Built-Using', '')
            try:
                get_relation(built_using, "${misc:Built-Using}")
            except KeyError:
                issue = LintianIssue(
                    updater.source,
                    'missing-built-using-field-for-golang-package',
                    binary['Package'])
                if issue.should_fix():
                    binary["Built-Using"] = add_dependency(
                        built_using, "${misc:Built-Using}")
                    added.append(binary['Package'])
                    issue.report_fixed()

if added and removed:
    report_result(
        'Added ${misc:Built-Using} to %s and removed it from %s.' %
        (', '.join(added), ', '.join(removed)))

if added:
    report_result(
        'Add missing ${misc:Built-Using} to Built-Using on %s.' %
        ', '.join(added))
if removed:
    report_result(
        'Remove unnecessary ${misc:Built-Using} for %s' %
        ', '.join(removed))
