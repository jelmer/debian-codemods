#!/usr/bin/python3

import asyncio
from debian.deb822 import PkgRelation
from lintian_brush.rules import update_rules
import os

import re


compat_release = os.environ.get('COMPAT_RELEASE', 'sid')
net_access = os.environ.get('NET_ACCESS', 'disallow') == 'allow'


def previous_release(release):
    import distro_info
    debian = distro_info.DebianDistroInfo()
    if release in (debian.devel(), debian.testing(), 'experimental'):
        return debian.stable()
    releases = debian.get_all()
    try:
        return releases[releases.index(release)-1]
    except ValueError:
        pass
    # TODO(jelmer): Ubuntu?
    return None


VERSION_CMP_SQL = {
    '=': '=',
    '>=': '>=',
    '<=': '<=',
    '>>': '>',
    '<<': '<',
    }


async def package_exists(package, release, version_cmp, version):
    if not net_access:
        try:
            return (
                package in
                os.environ['%s_PACKAGES' % release.upper()].split(','))
        except KeyError:
            return None
    from lintian_brush.udd import connect_udd_mirror
    udd = await connect_udd_mirror()
    row = await udd.fetchrow(
        'SELECT True FROM packages WHERE release = $2 AND package = $1 '
        'AND version %s $3' % VERSION_CMP_SQL[version_cmp],
        package, release, version)
    return bool(row)


def migration_done(rels):
    previous = previous_release(compat_release)
    if previous is None:
        # We can't determine if the migration is done
        return False
    for rel in rels:
        if len(rel) > 1:
            # Not sure how to handle | Replaces
            return False
        if asyncio.run(package_exists(
                rel[0]['name'], previous, *rel[0]['version'])) is not False:
            return False
    return True


def eliminate_dbgsym_migration(line, target):
    if not line.startswith(b'dh_strip'):
        return line

    def rep_dbgsym_migration(m):
        rep = m.group(2).strip(b'"').strip(b"'").decode()
        if '$' in rep:
            # too complicated
            return m.group(0)
        rep = PkgRelation.parse_relations(rep)
        if migration_done(rep):
            return b''
        return m.group(0)

    line = re.sub(
        b'([ \t]+)--dbgsym-migration[= ](\'[^\']+\'|\"[^\"]+\"|[^ ]+)',
        rep_dbgsym_migration, line)

    return line


update_rules(eliminate_dbgsym_migration)

print('Drop transition for old debug package migration.')
print('Fixed-Lintian-Tags: debug-symbol-migration-possibly-complete')
