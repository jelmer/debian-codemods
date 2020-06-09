#!/usr/bin/python3
import sys
from debmutate.control import (
    ControlEditor,
    )
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    )
from lintian_brush.fixer import (
    current_package_version,
    report_result,
    )
from lintian_brush.rules import (
    check_cdbs,
    update_rules,
    )

minimum_version = "9.20160114"


dbg_packages = set()
dbg_migration_done = set()


with ControlEditor() as updater:
    for control in updater.binaries:
        # Delete the freeradius-dbg package from debian/control
        package = control["Package"]
        if package.endswith('-dbg'):
            if package.startswith('python'):
                # -dbgsym packages don't include _d.so files for the python
                # interpreter
                continue
            dbg_packages.add(control["Package"])
            control.clear()

    if not dbg_packages:
        # no debug packages found to remove
        sys.exit(0)

    ensure_minimum_debhelper_version(updater.source, minimum_version)


current_version = str(current_package_version())
migrate_version = "<< %s%s" % (
        current_version,
        '' if current_version.endswith('~') else '~')

rules_uses_variables = False


def migrate_dh_strip(line, target):
    global rules_uses_variables
    if line.startswith(b'dh_strip ') or line.startswith(b'dh '):
        for dbg_pkg in dbg_packages:
            if ('--dbg-package=%s' % dbg_pkg).encode('utf-8') in line:
                line = line.replace(
                        ('--dbg-package=%s' % dbg_pkg).encode('utf-8'),
                        ("--dbgsym-migration='%s (%s)'" % (
                            dbg_pkg, migrate_version)).encode('utf-8'))
                dbg_migration_done.add(dbg_pkg)
        if b'$' in line:
            rules_uses_variables = True
    return line


update_rules(migrate_dh_strip)

difference = dbg_packages.symmetric_difference(dbg_migration_done)

if difference:
    if check_cdbs():
        # Ah, cdbs.
        raise Exception("package uses cdbs")
    if rules_uses_variables:
        # Don't know how to deal with these yet.
        sys.exit(2)
    raise Exception("packages missing %r" % difference)

report_result(
    "Transition to automatic debug package%s (from: %s)." %
    (("s" if len(dbg_packages) > 1 else ""), ', '.join(dbg_packages)),
    fixed_lintian_tags=['debian-control-has-obsolete-dbg-package'])
