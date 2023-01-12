#!/usr/bin/python3

# Find a list of tags that might qualify for inclusion in
# INFO_FIXERS in lintian_brush/lintian_overrides.py

import argparse

import psycopg2

from lintian_brush.lintian_overrides import INFO_FIXERS

parser = argparse.ArgumentParser()
args = parser.parse_args()

conn = psycopg2.connect(
    "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net/udd")
cursor = conn.cursor()
cursor.execute(
    "SELECT package_type, package, package_version, information FROM lintian "
    "WHERE tag = 'mismatched-override'")


tag_count = {}
for (_pkg_type, _pkg, _version, info) in cursor.fetchall():
    tag = info.split(' ')[0]
    tag_count.setdefault(tag, 0)
    tag_count[tag] += 1


tags_with_location_info = set()

cursor.execute("SELECT tag FROM lintian WHERE information LIKE '%[%]'")
for tag, in cursor.fetchall():
    tags_with_location_info.add(tag)


for tag, count in sorted(tag_count.items(), reverse=True, key=lambda k: k[1]):
    if tag not in tags_with_location_info:
        # Looks like there's no location info in this tag's info
        continue
    if tag in INFO_FIXERS:
        # We already have a fixer
        continue
    print(f'{tag:50}  {count}')
