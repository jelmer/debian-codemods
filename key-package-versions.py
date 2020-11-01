#!/usr/bin/python3

import json
from typing import Dict

KEY_PACKAGES = ('debhelper', 'dpkg')

OUTPUT_FILENAME = 'key-package-versions.json'

versions: Dict[str, Dict[str, str]] = {k: {} for k in KEY_PACKAGES}

with open(OUTPUT_FILENAME, 'r') as f:
    versions = json.load(f)


def update_debian(versions, key_packages):
    import psycopg2
    conn = psycopg2.connect(
        "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net/udd")

    cursor = conn.cursor()
    cursor.execute(
        "SELECT source, release, version from sources where source IN %s",
        (key_packages, ))

    for row in cursor.fetchall():
        versions[row[0]][row[1]] = row[2]


def update_ubuntu(versions, key_packages):
    from launchpadlib.launchpad import Launchpad
    lp = Launchpad.login_anonymously('lintian-brush')
    ubuntu = lp.distributions['ubuntu']
    archive = ubuntu.main_archive
    for series in ubuntu.series:
        print('  .. %s' % series.name)
        for pkg in key_packages:
            if (ubuntu.current_series.name != series.name and
                    series.name in versions[pkg]):
                continue
            ps = archive.getPublishedSources(
                exact_match=True, source_name=pkg, distro_series=series)
            versions[pkg][series.name] = ps[0].source_package_version


print('Downloading Debian key package information')
update_debian(versions, KEY_PACKAGES)

print('Downloading Ubuntu key package information')
update_ubuntu(versions, KEY_PACKAGES)


with open(OUTPUT_FILENAME, 'w') as f:
    json.dump(versions, f, indent=4, sort_keys=True)
