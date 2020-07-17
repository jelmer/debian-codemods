#!/usr/bin/python3

import os
import re
from debian.changelog import Changelog
from debmutate.control import ControlEditor, get_relation
from debian.copyright import Copyright, NotMachineReadableError
from debian.deb822 import Deb822
from lintian_brush.fixer import report_result

# Dictionary mapping source and target versions
upgrade_path = {
    "4.1.0": "4.1.1",
    "4.1.4": "4.1.5",
    "4.2.0": "4.2.1",
    "4.3.0": "4.4.0",
    "4.4.0": "4.4.1",
    "4.4.1": "4.5.0",
}


def check_4_1_1():
    return os.path.exists("debian/changelog")


def check_4_4_0():
    # Check that the package uses debhelper.
    if os.path.exists("debian/compat"):
        return True
    with open('debian/control') as f:
        source = next(Deb822.iter_paragraphs(f))
        build_deps = source.get('Build-Depends', '')
        try:
            get_relation(build_deps, 'debhelper-compat')
        except KeyError:
            return False
        else:
            return True


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
        return False

    # Check that Files entries don't refer to directories.
    # They must be wildcards *in* the directories.
    try:
        with open('debian/copyright', 'r') as f:
            copyright = Copyright(f, strict=False)
            for para in copyright.all_files_paragraphs():
                for glob in para.files:
                    if os.path.isdir(glob):
                        return False
    except FileNotFoundError:
        return False
    except NotMachineReadableError:
        pass
    return True


def check_4_1_5():
    # If epoch has changed -> return False
    with open('debian/changelog', 'r') as f:
        cl = Changelog(f, max_blocks=2)
        epochs = set()
        for block in cl:
            epochs.add(block.version.epoch)
        if len(epochs) > 1:
            return False

    with open('debian/control') as f:
        source = Deb822(f)
        if 'Rules-Requires-Root' not in source:
            return False

    return True


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
            return False
        if _poor_grep(entry.path, b'update-rc.d'):
            uses_update_rc_d = True

    # Including an init script is encouraged if there is no systemd unit, and
    # optional if there is (previously, it was recommended).
    for entry in os.scandir('debian'):
        if not entry.name.endswith('.init'):
            continue
        shortname = entry.name[:-len('.init')]
        if (not os.path.exists('debian/%s.service' % shortname) and
                not os.path.exists('debian/%s@.service' % shortname)):
            return False
        # Use of update-rc.d is required if the package includes an init
        # script.
        if not uses_update_rc_d:
            return False
    return True


check_requirements = {
    "4.1.1": check_4_1_1,
    "4.4.0": check_4_4_0,
    "4.4.1": check_4_4_1,
    "4.1.5": check_4_1_5,
    "4.5.0": check_4_5_0,
}

current_version = None


with ControlEditor() as updater:
    try:
        current_version = updater.source["Standards-Version"]
    except KeyError:
        # Huh, no standards version?
        pass
    else:
        while current_version in upgrade_path:
            target_version = upgrade_path[current_version]
            try:
                check_fn = check_requirements[target_version]
            except KeyError:
                pass
            else:
                if not check_fn():
                    break
            current_version = target_version
        updater.source["Standards-Version"] = current_version


if current_version:
    report_result(
        'Update standards version to %s, no changes needed.' % current_version,
        certainty='certain',
        fixed_lintian_tags=['out-of-date-standards-version'])
