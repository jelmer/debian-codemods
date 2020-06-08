#!/usr/bin/python3

from lintian_brush.control import ControlUpdater
from lintian_brush.fixer import report_result


with ControlUpdater() as updater:
    try:
        del updater.source["DM-Upload-Allowed"]
    except KeyError:
        pass


report_result(
    "Remove malformed and unnecessary DM-Upload-Allowed field in "
    "debian/control.",
    fixed_lintian_tags=['malformed-dm-upload-allowed'])
