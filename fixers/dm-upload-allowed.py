#!/usr/bin/python3

from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    try:
        old = updater.source["DM-Upload-Allowed"]
        del updater.source["DM-Upload-Allowed"]
    except KeyError:
        pass
    else:
        fixed_lintian_tag(
            updater.source, 'malformed-dm-upload-allowed',
            info=old)


report_result(
    "Remove malformed and unnecessary DM-Upload-Allowed field in "
    "debian/control.")
