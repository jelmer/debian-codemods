#!/usr/bin/python3

from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result


with ControlEditor() as updater:
    try:
        del updater.source["DM-Upload-Allowed"]
    except KeyError:
        pass


report_result(
    "Remove malformed and unnecessary DM-Upload-Allowed field in "
    "debian/control.",
    fixed_lintian_tags=['malformed-dm-upload-allowed'])
