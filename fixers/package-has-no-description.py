#!/usr/bin/python3

import sys
import textwrap

from lintian_brush.fixer import (
    control,
    LintianIssue,
    meets_minimum_certainty,
    net_access_allowed,
    report_result,
    trust_package,
    )
from upstream_ontologist.guess import guess_upstream_metadata

CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


def textwrap_description(text):
    import textwrap
    ret = []
    paras = text.split('\n\n')
    for para in paras:
        if '\n*' in para:
            ret.extend(para.splitlines())
        else:
            ret.extend(textwrap.wrap(para))
    return ret


def guess_description(binary_name, all_binaries, summary=None):
    if len(all_binaries) != 1:
        # TODO(jelmer): Support handling multiple binaries
        return None
    upstream_metadata = guess_upstream_metadata(
        '.', trust_package(), net_access_allowed())
    if summary is None:
        try:
            summary = upstream_metadata['X-Summary']
        except KeyError:
            return None
    try:
        upstream_description = textwrap_description(upstream_metadata['X-Description'])
    except KeyError:
        # Better than nothing..
        return summary
    lines = [line if line else '.' for line in upstream_description]
    description = summary + "\n" + ''.join([" %s\n" % line for line in lines])
    return description.rstrip('\n')


updated = []

with control as updater:
    for binary in updater.binaries:
        existing_description = binary.get('Description')
        if not existing_description:
            issue = LintianIssue(binary, 'required-field', 'Description')
            summary = None
        elif len(existing_description.splitlines()) == 1:
            issue = LintianIssue(binary, 'extended-description-is-empty')
            summary = existing_description.splitlines()[0]
        else:
            continue
        if not issue.should_fix():
            continue
        description = guess_description(
            binary['Package'], updater.binaries, summary=summary)
        if description and description != existing_description:
            binary['Description'] = description
            updated.append(binary['Package'])
            issue.report_fixed()


report_result(
    description='Add description for binary packages: %s' %
    ', '.join(sorted(updated)),
    certainty=CERTAINTY)
