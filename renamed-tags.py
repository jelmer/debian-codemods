#!/usr/bin/python3

# Extract renamed tags from lintian metadata.

from debian.deb822 import Deb822
import json
import os


renames = {}


def read_desc_files(path):
    for entry in os.scandir(path):
        if entry.is_dir():
            read_desc_files(entry.path)
        elif entry.name.endswith('.desc'):
            with open(entry.path, 'r') as f:
                desc = Deb822(f)
                for renamed_from in desc['Renamed-From']:
                    renames[renamed_from] = desc['Tag']


try:
    with open('/usr/share/lintian/data/override/renamed-tags', 'r') as f:
        for line in f:
            if line.startswith('#'):
                continue
            if not line.strip():
                continue
            (old, new) = line.split('=>')
            renames[old.strip()] = new.strip()
except FileNotFoundError:
    read_desc_files(renames, '/usr/share/lintian/tags')


with open('renamed-tags.json', 'w') as f:
    json.dump(renames, f, indent=4, sort_keys=True)
