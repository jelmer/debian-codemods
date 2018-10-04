#!/usr/bin/python
from io import BytesIO
from lintian_brush.control import update_control
from debian.deb822 import PkgRelation
from debian.changelog import Changelog, Version

minimum_version = Version("9.20160114")

def bump_debhelper(control):
    build_depends = PkgRelation.parse_relations(control["Build-Depends"])
    for relation in build_depends:
        names = [r['name'] for r in relation]
        if len(names) > 1 and names[0] == 'debhelper':
            raise Exception("Complex rule for debhelper, aborting")
        if names != ['debhelper']:
            continue
        if Version(relation[0]['version'][1]) < minimum_version:
            relation[0]['version'] = ('>=', minimum_version)
            control["Build-Depends"] = PkgRelation.str(build_depends)


dbg_packages = set()
dbg_migration_done = set()
def del_dbg(control):
    # Delete the freeradius-dbg package from debian/control
    if control["Package"].endswith('-dbg'):
        dbg_packages.add(control["Package"])
        control.clear()

update_control(source_package_cb=bump_debhelper, binary_package_cb=del_dbg)

with open('debian/changelog', 'rb') as f:
    cl = Changelog(f)

if cl.distributions == "UNRELEASED":
    version = "<< %s" % cl.version
else:
    version = "<= %s" % cl.version

outf = BytesIO()
with open('debian/rules', 'rb') as f:
    for l in f:
        if l.startswith('\tdh_strip '):
            for dbg_pkg in dbg_packages:
                if ('--dbg-package=%s' % dbg_pkg) in l:
                    l = l.replace('--dbg-package=%s' % dbg_pkg, "--dbgsym-migration='%s (%s)'" % (dbg_pkg, version)).encode('utf-8')
                    dbg_migration_done.add(dbg_pkg)
        outf.write(l)

if not dbg_packages:
    raise Exception("no debug packages found to remove")

difference = dbg_packages.symmetric_difference(dbg_migration_done)

if difference:
    raise Exception("packages missing %r" % difference)

with open('debian/rules', 'wb') as f:
    f.write(outf.getvalue())

print "Transition to automatic debug package%s (from: %s)." % (("s" if len(dbg_packages) > 1 else ""), ', '.join(dbg_packages))
