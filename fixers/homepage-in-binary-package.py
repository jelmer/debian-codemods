#!/usr/bin/python3

from lintian_brush.control import update_control

binary_homepages = set()
source_homepage = None


def update_source(source):
    global source_homepage
    source_homepage = source.get('Homepage')


def update_binary(binary):
    if 'Homepage' not in binary:
        return
    if source_homepage == binary['Homepage']:
        # Source and binary both have a homepage field, but they're the same =>
        # drop the binary package Homepage field
        del binary['Homepage']
    else:
        binary_homepages.add(binary['Homepage'])


update_control(
    binary_package_cb=update_binary,
    source_package_cb=update_source)

if source_homepage is None and binary_homepages:
    if len(binary_homepages) == 1:
        def set_source_homepage(control):
            control['Homepage'] = binary_homepages.pop()

        def drop_binary_homepage(control):
            if 'Homepage' in control:
                del control['Homepage']
        update_control(
            source_package_cb=set_source_homepage,
            binary_package_cb=drop_binary_homepage)


print('Set Homepage field in Source rather than Binary package.')
print('Fixed-Lintian-Tags: homepage-in-binary-package')
