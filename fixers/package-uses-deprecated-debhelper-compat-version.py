#!/usr/bin/python3
import os
import sys
from debian.changelog import Version
from debian.deb822 import Deb822
from lintian_brush.control import (
    drop_dependency,
    ensure_exact_version,
    ensure_minimum_version,
    get_relation,
    iter_relations,
    read_debian_compat_file,
    update_control,
    )
from lintian_brush.rules import (
    check_cdbs,
    dh_invoke_drop_with,
    dh_invoke_drop_argument,
    update_rules,
    )


# TODO(jelmer): Can we get these elsewhere rather than
# hardcoding them here?
MINIMUM_DEBHELPER_VERSION = 9

compat_release = os.environ.get('COMPAT_RELEASE', 'sid')

new_debhelper_compat_version = {
    # Debian
    'jessie': 9,
    'stretch': 10,
    'sid': 12,
    'bullseye': 12,
    'buster': 12,

    # Ubuntu
    'xenial': 9,
    'bionic': 11,
    'cosmic': 11,
    'disco': 12,
    'eoan': 12,
    }.get(compat_release, MINIMUM_DEBHELPER_VERSION)


if check_cdbs():
    # cdbs doesn't appear to support debhelper 11 or 12 just yet..
    new_debhelper_compat_version = min(new_debhelper_compat_version, 10)


if os.path.exists('debian/compat'):
    # Package currently stores compat version in debian/compat..

    current_debhelper_compat_version = read_debian_compat_file('debian/compat')

    # debhelper >= 11 supports the magic debhelper-compat build-dependency.
    # Exclude cdbs, since it only knows to get the debhelper compat version
    # from debian/compat.

    if new_debhelper_compat_version >= 11 and not check_cdbs():
        # Upgrade to using debhelper-compat, drop debian/compat file.
        os.unlink('debian/compat')

        # Assume that the compat version is set in Build-Depends
        def set_debhelper_compat(control):
            # TODO(jelmer): Use iter_relations rather than get_relation,
            # since that allows for complex debhelper rules.
            try:
                position, debhelper_relation = get_relation(
                    control.get("Build-Depends", ""), "debhelper")
            except KeyError:
                position = None
                debhelper_relation = []
            control["Build-Depends"] = ensure_exact_version(
                control.get("Build-Depends", ""), "debhelper-compat",
                "%d" % new_debhelper_compat_version, position=position)
            # If there are debhelper dependencies >= new debhelper compat
            # version, then keep them.
            for rel in debhelper_relation:
                if rel.version and Version(rel.version[1]) >= Version(
                        "%d" % new_debhelper_compat_version):
                    break
            else:
                control["Build-Depends"] = drop_dependency(
                    control.get("Build-Depends", ""), "debhelper")
                if control.get("Build-Depends") == "":
                    del control["Build-Depends"]

        update_control(source_package_cb=set_debhelper_compat)
    else:
        if current_debhelper_compat_version < new_debhelper_compat_version:
            with open('debian/compat', 'w') as f:
                f.write('%s\n' % new_debhelper_compat_version)
        else:
            # Nothing to do
            sys.exit(2)

        def bump_debhelper(control):
            control["Build-Depends"] = ensure_minimum_version(
                    control.get("Build-Depends", ""),
                    "debhelper",
                    "%d~" % new_debhelper_compat_version)

        update_control(source_package_cb=bump_debhelper)
else:
    # Assume that the compat version is set in Build-Depends
    def bump_debhelper_compat(control):
        global current_debhelper_compat_version
        try:
            offset, debhelper_compat_relation = get_relation(
                control.get("Build-Depends", ""), "debhelper-compat")
        except KeyError:
            sys.exit(2)
        else:
            if len(debhelper_compat_relation) > 1:
                # Not sure how to deal with this..
                sys.exit(2)
            if debhelper_compat_relation[0].version[0] != '=':
                # Not sure how to deal with this..
                sys.exit(2)
            current_debhelper_compat_version = Version(
                debhelper_compat_relation[0].version[1])
        if current_debhelper_compat_version < new_debhelper_compat_version:
            control["Build-Depends"] = ensure_exact_version(
                    control["Build-Depends"],
                    "debhelper-compat",
                    "%d" % new_debhelper_compat_version)

    update_control(source_package_cb=bump_debhelper_compat)


# For a list of behavior changes between debhelper compat verisons, see
# https://manpages.debian.org/testing/debhelper/debhelper.7.en.html#Supported_compatibility_levels


subitems = set()


def update_line(line, orig, new, description):
    newline = line.replace(orig, new)
    if newline != line:
        subitems.add(description)
    return newline


def update_line_drop_argument(target, line, command, argument, description):
    if (line.startswith(command + b' ') or
            (target == (b'override_' + command) and
             line.startswith(b'$(overridden_command)'))) and argument in line:
        line = dh_invoke_drop_argument(line, argument)
        subitems.add(description)
        return line, True
    return line, False


def upgrade_to_debhelper_12():

    def cb(line, target):
        line = update_line(
            line, b'dh_clean -k', b'dh_prep',
            'debian/rules: Replace dh_clean -k with dh_prep.')
        # TODO(jelmer): Also make sure that any extra arguments to
        # e.g. dh_auto_build get upgraded. E.g.
        # this works with disutils but not pybuild:
        # dh_auto_build -- --executable=/usr/bin/python
        # TODO(jelmer): Also add a dependency on dh-python
        line = update_line(
            line, b'--buildsystem=python_distutils', b'--buildsystem=pybuild',
            'Replace python_distutils buildsystem with pybuild.')
        line = update_line(
            line, b'-O--buildsystem=python_distutils',
            b'-O--buildsystem=pybuild',
            'Replace python_distutils buildsystem with pybuild.')
        if line.startswith(b'dh ') and b'buildsystem' not in line:
            with open('debian/control', 'r') as f:
                deb822 = Deb822(f.read())
            build_depends = deb822.get('Build-Depends', '')
            if any(iter_relations(build_depends, 'dh-python')):
                line += b' --buildsystem=pybuild'
        if line.startswith(b'dh ') or line.startswith(b'dh_installinit'):
            line = update_line(
                line, b'--no-restart-on-upgrade',
                b'--no-stop-on-upgrade',
                'Replace --no-restart-on-upgrade with --no-stop-on-upgrade.')
        line, changed = update_line_drop_argument(
            target, line, b'dh_install', b'--list-missing',
            'debian/rules: Call dh_missing rather than using dh_install '
            '--list-missing.')
        line, changed = update_line_drop_argument(
            target, line, b'dh_install', b'--fail-missing',
            'debian/rules: Move --fail-missing argument to dh_missing.')
        if changed:
            return [line, b'dh_missing --fail-missing']
        return line

    update_rules(cb)


def upgrade_to_debhelper_11():

    def cb(line, target):
        line = dh_invoke_drop_with(line, b'systemd')
        if line.startswith(b'dh_systemd_enable'):
            line = update_line(
                line, b'dh_systemd_enable', b'dh_installsystemd',
                'Use dh_installsystemd rather than deprecated '
                'dh_systemd_enable.')
        if line.startswith(b'dh_systemd_start'):
            line = update_line(
                line, b'dh_systemd_start', b'dh_installsystemd',
                'Use dh_installsystemd rather than deprecated '
                'dh_systemd_start.')
        return line

    update_rules(cb)
    for name in os.listdir('debian'):
        parts = name.split('.')
        if len(parts) < 2 or parts[-1] != 'upstart':
            continue
        if len(parts) == 3:
            package = parts[0]
            service = parts[1]
        elif len(parts) == 2:
            package = service = parts[0]
        os.unlink(os.path.join('debian', name))
        subitems.add('Drop obsolete upstart file %s.' % name)
        with open('debian/%s.maintscript' % package, 'a') as f:
            f.write('rm_conffile /etc/init/%s.conf %s\n' % (
                service, os.environ['CURRENT_VERSION']))


upgrade_to_debhelper = {
    11: upgrade_to_debhelper_11,
    12: upgrade_to_debhelper_12,
}


for version in range(int(str(current_debhelper_compat_version)),
                     int(str(new_debhelper_compat_version))+1):
    try:
        upgrade_to_debhelper[version]()
    except KeyError:
        pass

if new_debhelper_compat_version > current_debhelper_compat_version:
    if current_debhelper_compat_version < MINIMUM_DEBHELPER_VERSION:
        kind = "deprecated"
        tag = "package-uses-deprecated-debhelper-compat-version"
    else:
        kind = "old"
        tag = "package-uses-old-debhelper-compat-version"
    print("Bump debhelper from %s %s to %s." % (
        kind, current_debhelper_compat_version, new_debhelper_compat_version))
    for subitem in sorted(subitems):
        print("+ " + subitem)
    print("Fixed-Lintian-Tags: %s" % tag)
else:
    print("Set debhelper-compat version in Build-Depends.")
