#!/usr/bin/python3

from debmutate.control import ControlEditor, delete_from_list

from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    if 'Uploaders' in updater.source:
        uploaders = updater.source['Uploaders'].split(',')
        maintainer = updater.source['Maintainer']
        if maintainer in [uploader.strip() for uploader in uploaders]:
            updater.source['Uploaders'] = delete_from_list(
                updater.source['Uploaders'], maintainer)
            fixed_lintian_tag(
                updater.source, 'maintainer-also-in-uploaders')


report_result("Remove maintainer from uploaders.")
