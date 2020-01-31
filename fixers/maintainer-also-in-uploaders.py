#!/usr/bin/python3

from lintian_brush.control import ControlUpdater, delete_from_list


with ControlUpdater() as updater:
    if 'Uploaders' in updater.source:
        uploaders = updater.source['Uploaders'].split(',')
        maintainer = updater.source['Maintainer']
        if maintainer in [uploader.strip() for uploader in uploaders]:
            updater.source['Uploaders'] = delete_from_list(
                updater.source['Uploaders'], maintainer)


print("Remove maintainer from uploaders.")
print("Fixed-Lintian-Tags: maintainer-also-in-uploaders")
