#!/usr/bin/python3

from email.utils import parseaddr
from typing import Dict, List, Optional, Tuple

from lintian_brush.fixer import control, fixed_lintian_tag, report_result

REPLACEMENTS: Dict[Optional[str], Tuple[str, List[Tuple[str, str]]]] = {
    "python-modules-team@lists.alioth.debian.org": (
        "old-dpmt-vcs",
        [
            (
                "https://salsa.debian.org/python-team/modules/",
                "https://salsa.debian.org/python-team/packages/",
            )
        ],
    ),
    "python-apps-team@lists.alioth.debian.org": (
        "old-papt-vcs",
        [
            (
                "https://salsa.debian.org/python-team/applications/",
                "https://salsa.debian.org/python-team/packages/",
            )
        ],
    ),
}

with control as editor:
    maint: Optional[str]
    email: Optional[str]
    try:
        maint, email = parseaddr(editor.source["Maintainer"])
    except KeyError:
        maint, email = None, None
    changed_fields = set()
    try:
        tag, replacements = REPLACEMENTS[email]
    except KeyError:
        pass
    else:
        for field in [f for f in editor.source if f.startswith("Vcs-")]:
            url = editor.source[field]
            for old, new in replacements:
                url = url.replace(old, new)
            if url != editor.source[field]:
                editor.source[field] = url
                changed_fields.add(field)
                fixed_lintian_tag(editor.source, tag, info="")

    report_result(
        "Update fields {} for maintainer {}.".format(
            ", ".join(sorted(changed_fields)), maint
        )
    )
