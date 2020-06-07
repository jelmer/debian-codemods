#!/usr/bin/python3

from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result

updated_packages = set()


with ControlEditor() as updater:
    for binary in updater.binaries:
        package = binary['Package']
        if (not package.startswith('fonts-') and
                not package.startswith('xfonts-')):
            continue
        if binary.get('Architecture') not in ('all', None):
            continue
        if 'Multi-Arch' in binary:
            continue
        binary['Multi-Arch'] = 'foreign'
        updated_packages.add(package)


report_result(
    'Set Multi-Arch: foreign on package%s %s.' % (
        's' if len(updated_packages) > 1 else '', ', '.join(updated_packages)),
    fixed_lintian_tags=['font-package-not-multi-arch-foreign'])
