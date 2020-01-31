#!/usr/bin/python3

from lintian_brush.control import ControlUpdater


with ControlUpdater() as updater:
    try:
        del updater.source["DM-Upload-Allowed"]
    except KeyError:
        pass


print("Remove malformed and unnecessary DM-Upload-Allowed field in "
      "debian/control.")
print("Fixed-Lintian-Tags: malformed-dm-upload-allowed")
