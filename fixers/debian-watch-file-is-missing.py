#!/usr/bin/python3
import os
import sys

from debmutate.watch import WatchFile

from lintian_brush import (
    certainty_to_confidence,
    )
from lintian_brush.fixer import (
    current_package_version,
    net_access_allowed,
    report_result,
    package_is_native,
    LintianIssue,
    )
from lintian_brush.watch import (
    candidates_from_setup_py,
    candidates_from_upstream_metadata,
    )


if os.path.exists('debian/watch') or package_is_native():
    # Nothing to do here..
    sys.exit(0)

issue = LintianIssue('source', 'debian-watch-file-is-missing', info='')

if not issue.should_fix():
    sys.exit(0)

candidates = []

good_upstream_versions = {current_package_version().upstream_version}

if os.path.exists('setup.py'):
    candidates.extend(candidates_from_setup_py(
        'setup.py', good_upstream_versions,
        net_access=net_access_allowed()))

if os.path.exists('debian/upstream/metadata'):
    candidates.extend(candidates_from_upstream_metadata(
        'debian/upstream/metadata', good_upstream_versions,
        net_access=net_access_allowed()))


def candidate_key(candidate):
    return (
        certainty_to_confidence(candidate.certainty),
        candidate.preference)


candidates.sort(key=candidate_key)

# TODO(jelmer): parse cabal file and call candidates_from_hackage

if not candidates:
    sys.exit(0)

winner = candidates[0]

wf = WatchFile()
wf.entries.append(winner.watch)

with open('debian/watch', 'w') as f:
    wf.dump(f)
    issue.report_fixed()

report_result(
    "Add debian/watch file, using %s." % winner.site,
    certainty=winner.certainty)
