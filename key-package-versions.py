#!/usr/bin/python3

from debian.changelog import Version
import json
import os
from typing import Dict

KEY_PACKAGES = ('debhelper', 'dpkg')

OUTPUT_FILENAME = 'key-package-versions.json'

DEFAULT_UDD_URL = (
    "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net/udd")

versions: Dict[str, Dict[str, str]]

with open(OUTPUT_FILENAME) as f:
    versions = json.load(f)


for kp in KEY_PACKAGES:
    versions.setdefault(kp, {})


def update_debian(versions, key_packages):
    import psycopg2
    conn = psycopg2.connect(
        os.environ.get('UDD_URL', DEFAULT_UDD_URL))

    cursor = conn.cursor()
    cursor.execute(
        "SELECT source, release, version from sources WHERE source IN %s",
        (key_packages, ))

    for row in cursor.fetchall():
        versions[row[0]][row[1]] = row[2]


def update_ubuntu(versions, key_packages):
    from launchpadlib.launchpad import Launchpad
    from launchpadlib.uris import LPNET_SERVICE_ROOT
    lp = Launchpad.login_anonymously(
        'lintian-brush', service_root=LPNET_SERVICE_ROOT)
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
            versions[pkg][series.name] = str(max(
                Version(p.source_package_version)
                for p in ps
                if p.pocket == 'Release'))


print('Downloading Debian key package information')
update_debian(versions, KEY_PACKAGES)

print('Downloading Ubuntu key package information')
update_ubuntu(versions, KEY_PACKAGES)


with open(OUTPUT_FILENAME, 'w') as f:
    json.dump(versions, f, indent=4, sort_keys=True)
