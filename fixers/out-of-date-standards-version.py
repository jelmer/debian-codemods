#!/usr/bin/python3

import os
from debian.changelog import Changelog
from lintian_brush.control import ControlUpdater, get_relation
from debian.copyright import Copyright, NotMachineReadableError
from debian.deb822 import Deb822

# Dictionary mapping source and target versions
upgrade_path = {
    "4.1.0": "4.1.1",
    "4.2.0": "4.2.1",
    "4.3.0": "4.4.0",
    "4.4.0": "4.4.1",
    "4.1.4": "4.1.5",
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
            copyright = Copyright(f)
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


check_requirements = {
    "4.1.1": check_4_1_1,
    "4.4.0": check_4_4_0,
    "4.4.1": check_4_4_1,
    "4.1.5": check_4_1_5,
}

current_version = None


with ControlUpdater() as updater:
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
    print('Update standards version to %s, no changes needed.' %
          current_version)
print('Certainty: certain')
print('Fixed-Lintian-Tags: out-of-date-standards-version')
