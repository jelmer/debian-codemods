#!/usr/bin/python3

import re

from lintian_brush.fixer import LintianIssue, report_result

with open("debian/copyright", "rb") as f:
    lines = list(f)

# Check for insecure debian.org copyright format URI
m = re.match(
    rb"^(Format|Format-Specification): "
    rb"(http:\/\/www.debian.org\/doc\/packaging-manuals\/"
    rb"copyright-format\/1.0.*)\n",
    lines[0],
)

# Check for wiki copyright format URI
m_wiki = re.match(
    rb"^(Format|Format-Specification): "
    rb"(http:\/\/wiki.debian.org\/Proposals\/CopyrightFormat.*)\n",
    lines[0],
)
if m or m_wiki:
    newline = (
        b"Format: https://www.debian.org/doc/packaging-manuals/"
        b"copyright-format/1.0/\n"
    )
    if newline != lines[0]:
        lines[0] = newline
        if m:
            issue = LintianIssue(
                "source", "insecure-copyright-format-uri", m.group(2).decode()
            )
            if issue.should_fix():
                with open("debian/copyright", "wb") as f:
                    f.writelines(lines)
                issue.report_fixed()
        else:  # m_wiki
            # For wiki URI, we fix both insecure-copyright-format-uri and wiki-copyright-format-uri
            issue1 = LintianIssue(
                "source",
                "insecure-copyright-format-uri",
                m_wiki.group(2).decode(),
            )
            issue2 = LintianIssue(
                "source", "wiki-copyright-format-uri", m_wiki.group(2).decode()
            )
            if issue1.should_fix() or issue2.should_fix():
                with open("debian/copyright", "wb") as f:
                    f.writelines(lines)
                if issue1.should_fix():
                    issue1.report_fixed()
                if issue2.should_fix():
                    issue2.report_fixed()

report_result("Use secure copyright file specification URI.")
