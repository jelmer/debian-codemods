#!/usr/bin/python3

import sys
from typing import List, Optional

from upstream_ontologist import UpstreamDatum
from upstream_ontologist.guess import guess_upstream_metadata

from lintian_brush.fixer import (
    LintianIssue,
    control,
    meets_minimum_certainty,
    net_access_allowed,
    report_result,
    trust_package,
)

CERTAINTY = "possible"

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


def textwrap_description(text: UpstreamDatum | str) -> List[str]:
    import textwrap

    if isinstance(text, UpstreamDatum):
        text = text.value

    ret = []
    paras = text.split("\n\n")
    for para in paras:
        if "\n*" in para:
            ret.extend(para.splitlines())
        else:
            ret.extend(textwrap.wrap(para))
        ret.append("")
    if not ret[-1]:
        ret.pop(-1)
    return ret


# TODO(jelmer): Use debmutate.control.format_description instead
def format_description(summary: UpstreamDatum | str, lines):
    if isinstance(summary, UpstreamDatum):
        summary = summary.value
    return summary + "\n" + "".join([f" {line}\n" for line in lines])


def guess_description(binary_name, all_binaries, summary=None):
    if len(all_binaries) != 1:
        # TODO(jelmer): Support handling multiple binaries
        return None

    upstream_metadata = guess_upstream_metadata(
        ".", trust_package(), net_access_allowed()
    )
    if summary is None:
        summary = upstream_metadata.get("Summary")
    try:
        upstream_description = textwrap_description(
            upstream_metadata["Description"]
        )
    except KeyError:
        # Better than nothing..
        return summary

    if summary is None:
        if len(upstream_description) == 1:
            return upstream_description[0].rstrip("\n")
        return None
    lines = [line if line else "." for line in upstream_description]

    return format_description(summary, lines)


updated = []

with control as updater:
    summary: Optional[str]
    for binary in updater.binaries:
        existing_description = binary.get("Description")
        if not existing_description:
            issue = LintianIssue(binary, "required-field", "Description")
            summary = None
        elif len(existing_description.strip().splitlines()) == 1:
            issue = LintianIssue(binary, "extended-description-is-empty")
            summary = existing_description.splitlines()[0]
        else:
            continue
        if not issue.should_fix():
            continue
        description = guess_description(
            binary["Package"], list(updater.binaries), summary=summary
        )
        if description and description != existing_description:
            binary["Description"] = description
            updated.append(binary["Package"])
            issue.report_fixed()


report_result(
    description="Add description for binary packages: {}".format(", ".join(sorted(updated))),
    certainty=CERTAINTY,
)
