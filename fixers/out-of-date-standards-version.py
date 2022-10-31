#!/usr/bin/python3

import os
import logging
import re
import sys
from debian.changelog import Changelog
from debmutate.control import (
    get_relation,
    parse_standards_version,
    )
from typing import Dict, List
from debian.copyright import Copyright, NotMachineReadableError
from debian.deb822 import Deb822
from lintian_brush.fixer import control, report_result, LintianIssue
from lintian_brush.standards_version import iter_standards_versions

# Dictionary mapping source and target versions
upgrade_path = {
    "4.1.0": "4.1.1",
    "4.1.4": "4.1.5",
    "4.2.0": "4.2.1",
    "4.3.0": "4.4.0",
    "4.4.0": "4.4.1",
    "4.4.1": "4.5.0",
    "4.5.0": "4.5.1",
    "4.5.1": "4.6.0",
    "4.6.0": "4.6.1",
}


class UpgradeCheckFailure(Exception):
    """Upgrade check failed."""

    def __init__(self, section, reason):
        self.section = section
        self.reason = reason


class UpgradeCheckUnable(Exception):
    """Unable to check upgrade requirement."""

    def __init__(self, section, reason):
        self.section = section
        self.reason = reason


def check_4_1_1():
    if not os.path.exists("debian/changelog"):
        raise UpgradeCheckFailure("4.4", "debian/changelog does not exist")
    else:
        yield "debian/changelog exists"


def check_4_4_0():
    # Check that the package uses debhelper.
    if os.path.exists("debian/compat"):
        yield "package uses debhelper"
        return
    with open('debian/control') as f:
        source = next(Deb822.iter_paragraphs(f))
        build_deps = source.get('Build-Depends', '')
        try:
            get_relation(build_deps, 'debhelper-compat')
        except KeyError:
            raise UpgradeCheckFailure("4.9", "package does not use dh")
        else:
            yield "package uses debhelper"
            return


def check_4_4_1():
    # Check that there is only one Vcs field.
    vcs_fields = []
    with open('debian/control') as f:
        source = next(Deb822.iter_paragraphs(f))
        for name in source:
            if name.lower() == 'vcs-browser':
                continue
            if name.lower().startswith('vcs-'):
                vcs_fields.append(name)
    if len(vcs_fields) > 1:
        raise UpgradeCheckFailure(
            "5.6.26", "package has more than one Vcs-<type> field")
    elif len(vcs_fields) == 0:
        yield "package has no Vcs-<type> fields"
    else:
        yield "package has only one Vcs-<type> field"

    # Check that Files entries don't refer to directories.
    # They must be wildcards *in* the directories.
    try:
        with open('debian/copyright', 'r') as f:
            copyright = Copyright(f, strict=False)
            for para in copyright.all_files_paragraphs():
                for glob in para.files:
                    if os.path.isdir(glob):
                        raise UpgradeCheckFailure(
                            "copyright-format",
                            "Wildcards are required to match the contents of "
                            "directories")
    except FileNotFoundError:
        pass
    except NotMachineReadableError:
        pass
    else:
        yield "Files entries in debian/copyright don't refer to directories"


def check_4_1_5():
    # If epoch has changed
    with open('debian/changelog', 'r') as f:
        cl = Changelog(f, max_blocks=2)
        epochs = set()
        for block in cl:
            epochs.add(block.version.epoch)
        if len(epochs) > 1:
            # Maybe they did email debian-devel@; we don't know.
            raise UpgradeCheckUnable("5.6.12", "last release changes epoch")
    yield "Package did not recently introduce epoch"


def _poor_grep(path, needle):
    with open(path, 'rb') as f:
        lines = f.readlines()
        pat = re.compile(needle)
        return any(bool(pat.search(line)) for line in lines)


def check_4_5_0():
    # Hardcoded or dynamically allocated usernames should start with an
    # underscore.

    uses_update_rc_d = False

    # For now, just check if there are any postinst / preinst script that call
    # adduser / useradd
    for entry in os.scandir('debian'):
        if (not entry.name.endswith('.postinst') and
                not entry.name.endswith('.preinst')):
            continue
        if _poor_grep(entry.path, b'(adduser|useradd)'):
            raise UpgradeCheckUnable(
                "9.2.1", "dynamically generated usernames should start with "
                "an underscore")
        if _poor_grep(entry.path, b'update-rc.d'):
            uses_update_rc_d = True
    yield "Package does not create users"

    # Including an init script is encouraged if there is no systemd unit, and
    # optional if there is (previously, it was recommended).
    for entry in os.scandir('debian'):
        if not entry.name.endswith('.init'):
            continue
        shortname = entry.name[:-len('.init')]
        if (not os.path.exists('debian/%s.service' % shortname) and
                not os.path.exists('debian/%s@.service' % shortname)):
            raise UpgradeCheckFailure(
                "9.3.1",
                "packages that include system services should include "
                "systemd units")
        # Use of update-rc.d is required if the package includes an init
        # script.
        if not uses_update_rc_d:
            raise UpgradeCheckFailure(
                "9.3.3",
                "update-rc usage if required if package includes init script")
    if uses_update_rc_d:
        yield ("Package does not ship any init files without matching "
               "systemd units")
        yield "Package ships init files but uses update-rc.d"
    else:
        yield "Package does not ship init files"


def check_4_5_1():
    # TODO(jelmer): check whether necessary copyright headers have been copied
    # verbatim into copyright file?

    try:
        for entry in os.scandir('debian/patches'):
            if entry.name.endswith('.series'):
                raise UpgradeCheckFailure(
                    "4.5.1",
                    "package contains non-default series file")
    except FileNotFoundError:
        yield "Package does not have any patches"
    else:
        yield "Package does not ship any non-default series files"


def check_4_2_1():
    yield from []


def check_4_6_0():
    # TODO(jelmer): No package is allowed to install files in /usr/lib64/.
    # Previously, this prohibition only applied to packages for 64-bit
    # architectures.
    for entry in os.scandir('debian'):
        if not entry.is_file():
            continue
        if _poor_grep(entry.path, b'lib64'):
            raise UpgradeCheckUnable(
                "9.1.1",
                "unable to verify whether "
                "package install files into /usr/lib/64")
    else:
        yield "Package does not contain any references to lib64"


def check_4_6_1():
    # 9.1.1: Restore permission for packages for non-64-bit architectures to
    # install files to /usr/lib64/.
    # -> No need to check anything.
    yield from []


check_requirements = {
    "4.1.1": check_4_1_1,
    "4.2.1": check_4_2_1,
    "4.4.0": check_4_4_0,
    "4.4.1": check_4_4_1,
    "4.1.5": check_4_1_5,
    "4.5.0": check_4_5_0,
    "4.5.1": check_4_5_1,
    "4.6.0": check_4_6_0,
    "4.6.1": check_4_6_1,
}

current_version = None


verified: Dict[str, List[str]] = {}

try:
    with control as updater:
        try:
            current_version = updater.source["Standards-Version"]
        except KeyError:
            # Huh, no standards version?
            sys.exit(0)
        else:
            try:
                svs = dict(iter_standards_versions())
            except FileNotFoundError:
                dt = None
                last = None
                tag = 'out-of-date-standards-version'
            else:
                last, last_dt = max(svs.items())
                try:
                    dt = svs[parse_standards_version(current_version)]
                except KeyError:
                    dt = None
                    tag = 'out-of-date-standards-version'
                else:
                    age = last_dt - dt
                    if age.days > 365 * 2:
                        tag = 'ancient-standards-version'
                    else:
                        tag = 'out-of-date-standards-version'
            issue = LintianIssue(
                updater.source,
                tag,
                '%s%s%s' % (
                    current_version,
                    (' (released %s)' % dt.strftime('%Y-%m-%d')) if dt else '',
                    (' (current is %s)' %
                     '.'.join([str(x) for x in last]))
                    if last is not None else '',
                    ))
            if issue.should_fix():
                while current_version in upgrade_path:
                    target_version = upgrade_path[current_version]
                    check_fn = check_requirements[target_version]
                    try:
                        verified[target_version] = list(check_fn())
                    except UpgradeCheckFailure as e:
                        logging.info(
                            'Upgrade checklist validation from standards '
                            '%s ⇒ %s failed: %s: %s',
                            current_version, target_version,
                            e.section, e.reason)
                        break
                    except UpgradeCheckUnable as e:
                        logging.info(
                            'Unable to validate checklist from standards '
                            '%s ⇒ %s: %s: %s',
                            current_version, target_version,
                            e.section, e.reason)
                        break
                    current_version = target_version
                updater.source["Standards-Version"] = current_version
                issue.report_fixed()
except FileNotFoundError:
    sys.exit(0)


if current_version:
    report_result(
        f'Update standards version to {current_version}, no changes needed.',
        certainty='certain')
