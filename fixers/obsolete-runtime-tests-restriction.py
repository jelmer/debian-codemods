#!/usr/bin/python3

import os
import sys

from debmutate.control import delete_from_list
from debmutate.deb822 import Deb822Editor

from lintian_brush.fixer import fixed_lintian_tag, report_result
from lintian_brush.lintian import LINTIAN_DATA_PATH

removed_restrictions = []


if not os.path.exists("debian/tests/control"):
    sys.exit(0)

KNOWN_OBSOLETE_RESTRICTIONS_PATH = os.path.join(
    LINTIAN_DATA_PATH, "testsuite/known-obsolete-restrictions"
)
DEPRECATED_RESTRICTIONS = []

try:
    with open(KNOWN_OBSOLETE_RESTRICTIONS_PATH) as f:
        for line in f:
            if line.startswith("#"):
                continue
            if not line.strip():
                continue
            DEPRECATED_RESTRICTIONS.append(line.strip())
except FileNotFoundError:
    sys.exit(2)

with Deb822Editor("debian/tests/control") as updater:
    for paragraph in updater.paragraphs:
        restrictions = paragraph.get("Restrictions", "").split(",")
        if restrictions == [""]:
            continue
        to_delete = []
        for restriction in list(restrictions):
            if restriction.strip() in DEPRECATED_RESTRICTIONS:
                to_delete.append(restriction.strip())
                fixed_lintian_tag(
                    "source",
                    "obsolete-runtime-tests-restriction",
                    f"{restriction.strip()} in line XX",
                )
        if to_delete:
            removed_restrictions.extend(to_delete)
            paragraph["Restrictions"] = delete_from_list(
                paragraph["Restrictions"], to_delete
            )
            if not paragraph["Restrictions"].strip():
                del paragraph["Restrictions"]


if "needs-recommends" in removed_restrictions:
    # This is Certainty: possible, since the package may actually rely on the
    # (direct? indirect?) recommends, in which case we'd want to add them to
    # Depends.
    certainty = "possible"
else:
    certainty = "certain"


report_result(
    "Drop deprecated restriction{} {}. See "
    "https://salsa.debian.org/ci-team/autopkgtest/tree/"
    "master/doc/README.package-tests.rst".format(
        "s" if len(removed_restrictions) > 1 else "",
        ", ".join(removed_restrictions),
    ),
    certainty=certainty,
)
