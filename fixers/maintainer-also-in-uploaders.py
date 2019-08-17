#!/usr/bin/python3

from lintian_brush.control import update_control, delete_from_list


def drop_maintainer_from_uploaders(control):
    if 'Uploaders' not in control:
        return
    uploaders = control['Uploaders'].split(',')
    maintainer = control['Maintainer']
    if maintainer not in [uploader.strip() for uploader in uploaders]:
        return
    control['Uploaders'] = delete_from_list(control['Uploaders'], maintainer)


update_control(source_package_cb=drop_maintainer_from_uploaders)

print("Remove maintainer from uploaders.")
print("Fixed-Lintian-Tags: maintainer-also-in-uploaders")
