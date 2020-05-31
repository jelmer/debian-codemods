#!/usr/bin/python3

import os

from debian.changelog import Version

from lintian_brush.control import (
    drop_dependency,
    ensure_exact_version,
    get_relation,
    read_debian_compat_file,
    ControlUpdater,
    )
from lintian_brush.fixer import report_result
from lintian_brush.rules import (
    check_cdbs,
    )


if os.path.exists('debian/compat'):
    # Package currently stores compat version in debian/compat..

    debhelper_compat_version = read_debian_compat_file('debian/compat')

    # debhelper >= 11 supports the magic debhelper-compat build-dependency.
    # Exclude cdbs, since it only knows to get the debhelper compat version
    # from debian/compat.

    if debhelper_compat_version >= 11 and not check_cdbs():
        # Upgrade to using debhelper-compat, drop debian/compat file.
        os.unlink('debian/compat')

        # Assume that the compat version is set in Build-Depends
        with ControlUpdater() as updater:
            # TODO(jelmer): Use iter_relations rather than get_relation,
            # since that allows for complex debhelper rules.
            try:
                position, debhelper_relation = get_relation(
                    updater.source.get("Build-Depends", ""), "debhelper")
            except KeyError:
                position = None
                debhelper_relation = []
            updater.source["Build-Depends"] = ensure_exact_version(
                updater.source.get("Build-Depends", ""), "debhelper-compat",
                "%d" % debhelper_compat_version, position=position)
            current_compat_version = Version("%d" % debhelper_compat_version)
            # If there are debhelper dependencies >> new debhelper compat
            # version, then keep them.
            for rel in debhelper_relation:
                if not rel.version:
                    continue
                if rel.version[0] in ('=', '>=') and Version(
                        rel.version[1]) > current_compat_version:
                    break
                if rel.version[0] == '>>' and Version(
                        rel.version[1]) >= current_compat_version:
                    break
            else:
                updater.source["Build-Depends"] = drop_dependency(
                    updater.source.get("Build-Depends", ""), "debhelper")
                if updater.source.get("Build-Depends") == "":
                    del updater.source["Build-Depends"]

report_result(
    "Set debhelper-compat version in Build-Depends.",
    fixed_lintian_tags=['uses-debhelper-compat-file'])
