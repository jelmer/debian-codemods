#!/usr/bin/python3

# Extract renamed and known tags from lintian metadata.

import json
import os

from debian.deb822 import Deb822

renames = {}
# Names lintian recognises: every current tag plus every name a tag was
# renamed from. An override for a name in neither set is an alien-tag.
known = set()


def read_desc_files(path):
    for entry in os.scandir(path):
        if entry.is_dir():
            read_desc_files(entry.path)
        elif entry.name.endswith(".tag"):
            with open(entry.path) as f:
                desc = Deb822(f)
                known.add(desc["Tag"])
                for renamed_from in desc.get("Renamed-From", "").splitlines():
                    if renamed_from.strip():
                        renames[renamed_from.strip()] = desc["Tag"]
                        known.add(renamed_from.strip())


read_desc_files("/usr/share/lintian/tags/")

path = os.path.dirname(os.path.realpath(__file__))

with open(os.path.join(path, "renamed-tags.json"), "w") as f:
    json.dump(renames, f, indent=4, sort_keys=True)

with open(os.path.join(path, "known-tags.json"), "w") as f:
    json.dump(sorted(known), f, indent=4)
    f.write("\n")
