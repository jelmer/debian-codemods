#!/usr/bin/python3

from debmutate.control import ControlEditor

binary_homepages = set()
source_homepage = None


with ControlEditor() as updater:
    source_homepage = updater.source.get('Homepage')
    for binary in updater.binaries:
        if 'Homepage' not in binary:
            continue
        if source_homepage == binary['Homepage']:
            # Source and binary both have a homepage field, but they're the
            # same => drop the binary package Homepage field
            del binary['Homepage']
        else:
            binary_homepages.add(binary['Homepage'])

    if source_homepage is None and binary_homepages:
        if len(binary_homepages) == 1:
            updater.source['Homepage'] = binary_homepages.pop()

            for binary in updater.binaries:
                if 'Homepage' in binary:
                    del binary['Homepage']


print('Set Homepage field in Source rather than Binary package.')
print('Fixed-Lintian-Tags: homepage-in-binary-package')
