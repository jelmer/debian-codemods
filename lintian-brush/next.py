#!/usr/bin/python3
# Report lintian tags that might be good candidates to implement fixers for.

import argparse

import psycopg2
from ruamel.yaml import YAML

from lintian_brush import fixable_lintian_tags

parser = argparse.ArgumentParser()
parser.add_argument(
    "--exclude",
    type=str,
    default="hard",
    help="Comma-separated list of difficulties to exclude.",
)
args = parser.parse_args()

conn = psycopg2.connect(
    "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net/udd"
)
with conn.cursor() as cursor:
    cursor.execute(
        "SELECT tag, COUNT(DISTINCT package) AS package_count, "
        "COUNT(*) AS tag_count from lintian "
        "WHERE tag_type NOT IN ('classification') GROUP BY 1 ORDER BY 2 DESC"
    )

    supported_tags = set(fixable_lintian_tags())

    yaml = YAML()
    with open("tag-status.yaml") as f:
        tag_status = yaml.load(f)

    per_tag_status = {}
    for entry in tag_status or []:
        per_tag_status[entry["tag"]] = entry

    exclude_difficulties = args.exclude.split(",")

    for tag, package_count, tag_count in cursor:
        if tag in supported_tags:
            continue
        difficulty = per_tag_status.get(tag, {}).get("difficulty", "unknown")
        if difficulty in exclude_difficulties:
            continue
        print(f"{tag} {difficulty} {package_count}/{tag_count}")
