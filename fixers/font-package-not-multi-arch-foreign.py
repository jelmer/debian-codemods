#!/usr/bin/python3

from lintian_brush.control import ControlUpdater

updated_packages = set()


with ControlUpdater() as updater:
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


print('Set Multi-Arch: foreign on package%s %s.' % (
    's' if len(updated_packages) > 1 else '', ', '.join(updated_packages)))
print("Fixed-Lintian-Tags: font-packge-not-multi-arch-foreign")
