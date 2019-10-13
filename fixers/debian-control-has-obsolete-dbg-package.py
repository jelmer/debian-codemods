#!/usr/bin/python3
import os
import sys
from lintian_brush.control import (
    ensure_minimum_debhelper_version,
    update_control,
    )
from lintian_brush.rules import (
    check_cdbs,
    update_rules,
    )

minimum_version = "9.20160114"


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_debhelper_version(
        control.get("Build-Depends", ""), minimum_version)


dbg_packages = set()
dbg_migration_done = set()


def del_dbg(control):
    # Delete the freeradius-dbg package from debian/control
    package = control["Package"]
    if package.endswith('-dbg'):
        if package.startswith('python'):
            # -dbgsym packages don't include _d.so files for the python
            # interpreter
            return
        dbg_packages.add(control["Package"])
        control.clear()


update_control(binary_package_cb=del_dbg)
if not dbg_packages:
    # no debug packages found to remove
    sys.exit(0)

update_control(source_package_cb=bump_debhelper)


current_version = os.environ["CURRENT_VERSION"]
migrate_version = "<< %s%s" % (
        current_version,
        '' if current_version.endswith('~') else '~')


def migrate_dh_strip(line, target):
    if line.startswith(b'dh_strip ') or line.startswith(b'dh '):
        for dbg_pkg in dbg_packages:
            if ('--dbg-package=%s' % dbg_pkg).encode('utf-8') in line:
                line = line.replace(
                        ('--dbg-package=%s' % dbg_pkg).encode('utf-8'),
                        ("--dbgsym-migration='%s (%s)'" % (
                            dbg_pkg, migrate_version)).encode('utf-8'))
                dbg_migration_done.add(dbg_pkg)
    return line


update_rules(migrate_dh_strip)

difference = dbg_packages.symmetric_difference(dbg_migration_done)

if difference:
    if check_cdbs():
        # Ah, cdbs.
        raise Exception("package uses cdbs")
    raise Exception("packages missing %r" % difference)

print("Transition to automatic debug package%s (from: %s)." %
      (("s" if len(dbg_packages) > 1 else ""), ', '.join(dbg_packages)))
print("Fixed-Lintian-Tags: debian-control-has-obsolete-dbg-package")
