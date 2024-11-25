#!/usr/bin/python3

from contextlib import suppress

from debmutate.copyright import CopyrightEditor, NotMachineReadableError

from debian.copyright import License
from lintian_brush.fixer import LintianIssue, report_result

typos = {
    "bsd-2": "BSD-2-Clause",
    "bsd-3": "BSD-3-Clause",
    "bsd-4": "BSD-4-Clause",
    "agpl3": "AGPL-3",
    "agpl3+": "AGPL-3+",
    "lgpl2.1": "LGPL-2.1",
    "lgpl2": "LGPL-2.0",
    "lgpl3": "LGPL-3.0",
}
for i in [1, 2, 3]:
    typos[f"gplv{i}"] = f"GPL-{i}"
    typos[f"gplv{i}+"] = f"GPL-{i}+"
    typos[f"gpl{i}"] = f"GPL-{i}"
    typos[f"gpl{i}+"] = f"GPL-{i}+"

renames = {}


def fix_shortname(copyright):
    for paragraph in copyright.all_paragraphs():
        if paragraph.license is None:
            continue
        try:
            new_name = typos[paragraph.license.synopsis]
        except KeyError:
            continue
        issue = LintianIssue(
            "source",
            "invalid-short-name-in-dep5-copyright",
            info=paragraph.license.synopsis,
        )
        if issue.should_fix():
            renames[paragraph.license.synopsis] = new_name
            paragraph.license = License(new_name, paragraph.license.text)
            issue.report_fixed()


with suppress(
    FileNotFoundError, NotMachineReadableError
), CopyrightEditor() as updater:
    fix_shortname(updater.copyright)

report_result(
    "Fix invalid short license name in debian/copyright ({})".format(
        ", ".join([f"{old} ⇒ {new}" for (old, new) in renames.items()])
    )
)
