#!/usr/bin/python3

# Extract renamed tags from lintian metadata.

import json
import os

from debian.deb822 import Deb822

renames = {}


def read_desc_files(path):
    for entry in os.scandir(path):
        if entry.is_dir():
            read_desc_files(entry.path)
        elif entry.name.endswith(".tag"):
            with open(entry.path) as f:
                desc = Deb822(f)
                for renamed_from in desc.get("Renamed-From", "").splitlines():
                    if renamed_from.strip():
                        renames[renamed_from.strip()] = desc["Tag"]


read_desc_files("/usr/share/lintian/tags/")


with open("renamed-tags.json", "w") as f:
    json.dump(renames, f, indent=4, sort_keys=True)
