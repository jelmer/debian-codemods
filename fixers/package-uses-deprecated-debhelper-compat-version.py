#!/usr/bin/python3
import os
import re
import shlex
import sys
import warnings

from debmutate.control import (
    ensure_exact_version,
    ensure_minimum_version,
    get_relation,
    ControlEditor,
    )
from debmutate.debhelper import (
    read_debhelper_compat_file,
    )
from lintian_brush.debhelper import (
    detect_debhelper_buildsystem,
    lowest_non_deprecated_compat_level,
    maximum_debhelper_compat_version,
    DEBHELPER_BUILD_STEPS,
    )
from lintian_brush.fixer import (
    compat_release,
    current_package_version,
    report_result,
    )
from lintian_brush.rules import (
    check_cdbs,
    dh_invoke_drop_with,
    dh_invoke_drop_argument,
    dh_invoke_replace_argument,
    update_rules,
    Makefile,
    )


new_debhelper_compat_version = maximum_debhelper_compat_version(
    compat_release())

uses_cdbs = check_cdbs()

if uses_cdbs:
    # cdbs doesn't appear to support debhelper 11 or 12 just yet..
    new_debhelper_compat_version = min(new_debhelper_compat_version, 10)


# If the package uses autoconf, configure doesn't contain --runstatedir, and
# specifies --without autoreconf, then we can't upgrade beyond debhelper 10. If
# we do, configure fails because debhelper >= 11 specifies --runstatedir.

# We could choose to drop the --without autoreconf, but we don't know why the
# maintainer chose to specify it.
def autoreconf_disabled():
    try:
        mf = Makefile.from_path('debian/rules')
        for line in mf.dump_lines():
            if re.findall(b'--without.*autoreconf', line):
                return True

        # Another way to disable dh_autoreconf is to add an empty override
        # rule.
        for rule in mf.iter_rules(b'override_dh_autoreconf'):
            if rule.commands():
                return False
        else:
            return True
    except FileNotFoundError:
        return False
    return False


if autoreconf_disabled():
    try:
        with open('configure', 'rb') as f:
            for line in f:
                if b'runstatedir' in line:
                    break
            else:
                new_debhelper_compat_version = min(
                    new_debhelper_compat_version, 10)
                warnings.warn(
                    'Not upgrading beyond debhelper %d, since the package '
                    'disables autoreconf but its configure does not provide '
                    '--runstatedir.' % new_debhelper_compat_version)
    except FileNotFoundError:
        pass


if os.path.exists('debian/compat'):
    # Package currently stores compat version in debian/compat..

    current_debhelper_compat_version = read_debhelper_compat_file(
        'debian/compat')
    if current_debhelper_compat_version < new_debhelper_compat_version:
        with open('debian/compat', 'w') as cf:
            cf.write('%s\n' % new_debhelper_compat_version)
    else:
        # Nothing to do
        sys.exit(2)

    with ControlEditor() as updater:
        updater.source["Build-Depends"] = ensure_minimum_version(
            updater.source.get("Build-Depends", ""),
            "debhelper",
            "%d~" % new_debhelper_compat_version)
else:
    # Assume that the compat version is set in Build-Depends
    with ControlEditor() as updater:
        try:
            offset, debhelper_compat_relation = get_relation(
                updater.source.get("Build-Depends", ""), "debhelper-compat")
        except KeyError:
            sys.exit(2)
        else:
            if len(debhelper_compat_relation) > 1:
                # Not sure how to deal with this..
                sys.exit(2)
            if debhelper_compat_relation[0].version[0] != '=':
                # Not sure how to deal with this..
                sys.exit(2)
            current_debhelper_compat_version = int(
                debhelper_compat_relation[0].version[1])
        if current_debhelper_compat_version < new_debhelper_compat_version:
            updater.source["Build-Depends"] = ensure_exact_version(
                    updater.source["Build-Depends"],
                    "debhelper-compat",
                    "%d" % new_debhelper_compat_version)


# For a list of behavior changes between debhelper compat verisons, see
# https://manpages.debian.org/testing/debhelper/debhelper.7.en.html#Supported_compatibility_levels


subitems = set()


def line_matches_command(target, line, command):
    if command is None:
        # Whatever
        return True

    if line.startswith(command + b' ') or line == command:
        return True

    if (target == (b'override_' + command) and
            line.startswith(b'$(overridden_command)')):
        return True

    return False


def update_line(line, orig, new, description):
    newline = line.replace(orig, new)
    if newline != line:
        subitems.add(description)
        changed = True
    else:
        changed = False
    return newline, changed


def update_line_drop_argument(target, line, command, argument, description):
    if line_matches_command(target, line, command) and argument in line:
        line = dh_invoke_drop_argument(line, argument)
        subitems.add(description)
        return line, True
    return line, False


def update_line_replace_argument(line, old, new, description):
    newline = dh_invoke_replace_argument(line, old, new)
    if newline != line:
        subitems.add(description)
        return newline, True
    return line, False


class PybuildUpgrader(object):

    def __init__(self):
        # Does the dh line specify --buildsystem=pybuild?
        self.upgraded = False

    def fix_line(self, line, target):
        """Upgrade from python_distutils to pybuild."""
        line, changed = update_line(
            line, b'--buildsystem=python_distutils', b'--buildsystem=pybuild',
            'Replace python_distutils buildsystem with pybuild.')
        line, changed = update_line(
            line, b'--buildsystem python_distutils', b'--buildsystem=pybuild',
            'Replace python_distutils buildsystem with pybuild.')
        line, changed = update_line(
            line, b'-O--buildsystem=python_distutils',
            b'-O--buildsystem=pybuild',
            'Replace python_distutils buildsystem with pybuild.')
        if target.decode() in DEBHELPER_BUILD_STEPS:
            step = target.decode()
        elif target.startswith(b'override_dh_auto_'):
            step = target[len(b'override_dh_auto_'):].decode()
        else:
            step = None
        if line.startswith(b'dh '):
            if b'buildsystem' not in line:
                buildsystem = detect_debhelper_buildsystem(step)
                if buildsystem == 'python_distutils':
                    line += b' --buildsystem=pybuild'
                    self.upgraded = True
            else:
                if b'buildsystem=pybuild' in line:
                    self.upgraded = True
        if (line.startswith(b'dh_auto_') and
            b' -- ' in line and
            (self.upgraded or
                re.match(b'--buildsystem[= ]pybuild', line) or
                detect_debhelper_buildsystem(step) == 'pybuild')):
            line, rest = line.split(b' -- ', 1)
            if step is None:
                step = line[len(b'dh_auto_'):].split(b' ', 1)[0].decode()
            line = (b'PYBUILD_' + step.upper().encode() + b'_ARGS=' +
                    shlex.quote(rest.decode()).encode() + b' ' + line)

        return line


def upgrade_to_dh_prep(line, target):
    """Replace 'dh_clean -k' with 'dh_prep."""
    line, changed = update_line(
        line, b'dh_clean -k', b'dh_prep',
        'debian/rules: Replace dh_clean -k with dh_prep.')
    return line


class DhMissingUpgrader(object):
    """Replace --list-missing / --fail-missing with dh_missing."""

    def __init__(self):
        self.need_override_missing = False

    def fix_line(self, line, target):
        for arg in [b'--list-missing', b'-O--list-missing']:
            for command in [b'dh_install', b'dh']:
                line, changed = update_line_drop_argument(
                    target, line, command, arg,
                    'debian/rules: Rely on default use of dh_missing rather '
                    'than using dh_install --list-missing.')
        for arg in [b'--fail-missing', b'-O--fail-missing']:
            for command in [b'dh_install', b'dh']:
                line, changed = update_line_drop_argument(
                    target, line, command, arg,
                    'debian/rules: Move --fail-missing argument to dh_missing.'
                    )
                if changed:
                    if target == b'override_dh_install' or command == b'dh':
                        self.need_override_missing = True
                    else:
                        subitems.add(
                            'debian/rules: Move --fail-missing argument '
                            'to dh_missing.')
                        return [line, b'dh_missing --fail-missing']
        return line

    def fix_makefile(self, mf):
        if not self.need_override_missing:
            return
        try:
            [rule] = list(mf.iter_rules(b'override_dh_missing'))
        except ValueError:
            rule = mf.add_rule(b'override_dh_missing')
            rule.append_command(b'dh_missing --fail-missing')
        else:
            for i, line in enumerate(rule.lines):
                if line.startswith(b'dh_missing '):
                    if b'--fail-missing' in line:
                        return
                    else:
                        rule.lines[i] += b' --fail-missing'
            else:
                raise Exception(
                    'override_dh_missing exists, but has no call to '
                    'dh_missing')


def replace_deprecated_same_arch(line, target):
    if not line.startswith(b'dh'):
        return line
    line, _ = update_line_replace_argument(
        line, b'-s', b'-a', 'Replace deprecated -s with -a.')
    line, _ = update_line_replace_argument(
        line, b'--same-arch', b'--arch',
        'Replace deprecated --same-arch with --arch.')
    return line


def upgrade_to_no_stop_on_upgrade(line, target):
    if line.startswith(b'dh ') or line.startswith(b'dh_installinit'):
        line, changed = update_line(
            line, b'--no-restart-on-upgrade',
            b'--no-stop-on-upgrade',
            'Replace --no-restart-on-upgrade with --no-stop-on-upgrade.')
    return line


def debhelper_argument_order(line, target):
    if line.startswith(b'dh '):
        args = line.split(b' ')
        for possible_va in [b'$*', b'$@', b'${@}']:
            try:
                x = args.index(possible_va)
            except ValueError:
                continue
            break
        else:
            return line
        val = args.pop(x)
        args.insert(1, val)
        return b' '.join(args)
    return line


def upgrade_to_debhelper_12():

    pybuild_upgrader = PybuildUpgrader()
    dh_missing_upgrader = DhMissingUpgrader()
    update_rules([
        debhelper_argument_order,
        replace_deprecated_same_arch,
        pybuild_upgrader.fix_line,
        upgrade_to_dh_prep,
        upgrade_to_no_stop_on_upgrade,
        dh_missing_upgrader.fix_line,
        ], makefile_cb=dh_missing_upgrader.fix_makefile)


def upgrade_to_installsystemd(line, target):
    line = dh_invoke_drop_with(line, b'systemd')
    if line.startswith(b'dh_systemd_enable'):
        line, changed = update_line(
            line, b'dh_systemd_enable', b'dh_installsystemd',
            'Use dh_installsystemd rather than deprecated '
            'dh_systemd_enable.')
    if line.startswith(b'dh_systemd_start'):
        line, changed = update_line(
            line, b'dh_systemd_start', b'dh_installsystemd',
            'Use dh_installsystemd rather than deprecated '
            'dh_systemd_start.')
    return line


def rename_installsystemd_target(rule):
    rule.rename_target(
        b'override_dh_systemd_enable', b'override_dh_installsystemd')
    rule.rename_target(
        b'override_dh_systemd_start', b'override_dh_installsystemd')


def upgrade_to_debhelper_10():

    # dh_installinit will no longer install a file named debian/package as an
    # init script.

    with ControlEditor() as updater:
        for binary in updater.binaries:
            name = binary['Package']
            if os.path.isfile(os.path.join('debian', name)):
                os.rename(os.path.join('debian', name),
                          os.path.join('debian', name + '.init'))
                subitems.add(
                    'Rename debian/%s to debian/%s.init.' % (name, name))


def upgrade_to_debhelper_11():

    update_rules(
        [upgrade_to_installsystemd], rule_cb=rename_installsystemd_target)
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
                service, str(current_package_version())))


def drop_dh_missing_fail(line, target):
    """Remove dh_missing --fail-missing."""
    # Note that this is not currently replacing plain calls to dh_missing with
    # empty an "override_dh_missing" rule.

    if not line.startswith(b'dh_missing '):
        return line
    argv = line.split(b' ')
    for arg in [b'--fail-missing', b'-O--fail-missing']:
        if arg in argv:
            # If we added the --fail-missing argument first, then
            # don't announce that here.
            try:
                subitems.remove(
                    'debian/rules: Move --fail-missing argument '
                    'to dh_missing.')
            except KeyError:
                subitems.add(
                    'debian/rules: Drop --fail-missing argument '
                    'to dh_missing, which is now the default.')
            else:
                subitems.add(
                    'debian/rules: Drop --fail-missing argument, '
                    'now the default.')
            argv.remove(arg)
    return b' '.join(argv)


def upgrade_to_debhelper_13():

    # dh_installtempfiles will handle d/tmpfile{,s}.  It prefer the latter
    # and warns about the former.

    with ControlEditor() as updater:
        for binary in updater.binaries:
            name = binary['Package']
            tmpfile = os.path.join('debian', name + ".tmpfile")
            if os.path.isfile(tmpfile):
                os.rename(tmpfile, tmpfile + 's')
                subitems.add(
                    'Rename %s to %ss.' % (tmpfile, tmpfile))
        if os.path.isfile(os.path.join('debian', "tmpfile")):
            tmpfile = os.path.join('debian', "tmpfile")
            os.rename(tmpfile, tmpfile + 's')
            subitems.add(
                'Rename %s to %ss.' % (tmpfile, tmpfile))

    update_rules([
        # The dh_missing command will now default to --fail-missing.  This can
        # be reverted to a non-fatal warning by explicitly passing
        # --list-missing like it was in compat 12.
        drop_dh_missing_fail,
        ])


upgrade_to_debhelper = {
    10: upgrade_to_debhelper_10,
    11: upgrade_to_debhelper_11,
    12: upgrade_to_debhelper_12,
    13: upgrade_to_debhelper_13,
}


for version in range(int(str(current_debhelper_compat_version))+1,
                     int(str(new_debhelper_compat_version))+1):
    try:
        upgrade_to_debhelper[version]()
    except KeyError:
        pass

if new_debhelper_compat_version > current_debhelper_compat_version:
    if current_debhelper_compat_version < lowest_non_deprecated_compat_level():
        kind = "deprecated"
        tag = "package-uses-deprecated-debhelper-compat-version"
    else:
        kind = "old"
        tag = "package-uses-old-debhelper-compat-version"
    lines = ["Bump debhelper from %s %s to %s." % (
        kind, current_debhelper_compat_version, new_debhelper_compat_version)]
    for subitem in sorted(subitems):
        lines.append("+ " + subitem)

    report_result(
        description='\n'.join(lines),
        fixed_lintian_tags=[tag])
