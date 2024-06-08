#!/usr/bin/python3

import sys
from typing import Optional

from debmutate.changelog import ChangelogEditor, Version
from lintian_brush.debbugs import find_archived_wnpp_bugs, find_wnpp_bugs
from lintian_brush.fixer import net_access_allowed, report_result

versioned_changed: Optional[Version] = None


if not net_access_allowed():
    sys.exit(0)


version_changed = None


with ChangelogEditor() as editor:
    if editor.changelog[-1].bugs_closed:
        sys.exit(0)

    wnpp_bugs = find_wnpp_bugs(editor.changelog[-1].package)
    if wnpp_bugs:
        certainty = "certain"
    else:
        wnpp_bugs = find_archived_wnpp_bugs(editor.changelog[-1].package)
        certainty = "confident"

    if not wnpp_bugs:
        sys.exit(0)

    for i, line in enumerate(editor.changelog[-1]._changes):
        if not line:
            continue

        if "Initial release" in line:
            if not line.rstrip().endswith("."):
                line = line.rstrip() + "."
            editor.changelog[-1]._changes[i] = line + (
                " Closes: #{}".format(", ".join([str(bugno) for (bugno, kind) in wnpp_bugs]))
            )
            version_changed = editor.changelog[-1].version
            break


if version_changed:
    report_result(
        "Add {} bugs in {}.".format(
            ", ".join(sorted({kind for (bugno, kind) in wnpp_bugs})),
            version_changed,
        ),
        certainty=certainty,
    )
