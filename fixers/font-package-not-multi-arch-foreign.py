#!/usr/bin/python3

from lintian_brush.control import update_control

updated_packages = set()


def set_multiarch_foreign(binary):
    package = binary['Package']
    if (not package.startswith('fonts-') and
            not package.startswith('xfonts-')):
        return
    if binary.get('Architecture') not in ('all', None):
        return
    if 'Multi-Arch' in binary:
        return
    binary['Multi-Arch'] = 'foreign'
    updated_packages.add(package)


update_control(binary_package_cb=set_multiarch_foreign)

print('Set Multi-Arch: foreign on package%s %s.' % (
    's' if len(updated_packages) > 1 else '', ', '.join(updated_packages)))
print("Fixed-Lintian-Tags: font-packge-not-multi-arch-foreign")
