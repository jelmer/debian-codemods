#!/usr/bin/python3

from lintian_brush.control import update_control


def drop_dm_upload_allowed(source):
    try:
        del source["DM-Upload-Allowed"]
    except KeyError:
        pass


update_control(source_package_cb=drop_dm_upload_allowed)

print("Remove malformed and unnecessary DM-Upload-Allowed field in "
      "debian/control.")
print("Fixed-Lintian-Tags: malformed-dm-upload-allowed")
