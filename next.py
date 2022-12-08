#!/usr/bin/python3
# Report lintian tags that might be good candidates to implement fixers for.

import argparse

from lintian_brush import available_lintian_fixers
import psycopg2
from ruamel.yaml import YAML

parser = argparse.ArgumentParser()
parser.add_argument('--exclude', type=str, default='hard',
                    help='Comma-separated list of difficulties to exclude.')
args = parser.parse_args()

conn = psycopg2.connect(
    "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net/udd")
cursor = conn.cursor()
cursor.execute(
    "SELECT tag, COUNT(DISTINCT package) AS package_count, "
    "COUNT(*) AS tag_count from lintian "
    "WHERE tag_type NOT IN ('classification') GROUP BY 1 ORDER BY 2 DESC")

supported_tags = set()
for fixer in available_lintian_fixers():
    supported_tags.update(fixer.lintian_tags)

yaml = YAML()
with open('tag-status.yaml', 'r') as f:
    tag_status = yaml.load(f)

per_tag_status = {}
for entry in tag_status or []:
    per_tag_status[entry['tag']] = entry


exclude_difficulties = args.exclude.split(',')

for (tag, package_count, tag_count) in cursor.fetchall():
    if tag in supported_tags:
        continue
    difficulty = per_tag_status.get(tag, {}).get('difficulty', 'unknown')
    if difficulty in exclude_difficulties:
        continue
    print('%s %s %d/%d' % (tag, difficulty, package_count, tag_count))
