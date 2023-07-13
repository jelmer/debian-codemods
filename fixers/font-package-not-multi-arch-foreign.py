#!/usr/bin/python3

from lintian_brush.fixer import LintianIssue, control, report_result

updated_packages = set()


with control as updater:
    for binary in updater.binaries:
        package = binary['Package']
        if (not package.startswith('fonts-') and
                not package.startswith('xfonts-')):
            continue
        if binary.get('Architecture') not in ('all', None):
            continue
        if 'Multi-Arch' in binary:
            continue
        issue = LintianIssue(
            updater.source, 'font-package-not-multi-arch-foreign')
        if issue.should_fix():
            binary['Multi-Arch'] = 'foreign'
            updated_packages.add(package)
            issue.report_fixed()


report_result(
    'Set Multi-Arch: foreign on package{} {}.'.format(
        's' if len(updated_packages) > 1 else '', ', '.join(updated_packages)))
