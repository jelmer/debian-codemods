#!/usr/bin/python3
import os
import sys
from typing import Optional, Tuple

from debmutate.watch import WatchFile, Watch

from lintian_brush import (
    certainty_to_confidence,
    )
from lintian_brush.fixer import (
    current_package_version,
    net_access_allowed,
    report_result,
    package_is_native,
    )
from lintian_brush.watch import (
    candidates_from_setup_py,
    candidates_from_upstream_metadata,
    )


if os.path.exists('debian/watch') or package_is_native():
    # Nothing to do here..
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

# TODO(jelmer): parse cabal file and call candidates_from_hackage

if not candidates:
    sys.exit(0)

winner: Optional[Tuple[Watch, str, str]] = None
for candidate in candidates:
    if winner is not None and (  # type: ignore
            certainty_to_confidence(candidate[2]) >=   # type: ignore
            certainty_to_confidence(winner[2])):
        continue
    winner = candidate

if not winner:
    sys.exit(0)

wf = WatchFile()
(entry, site, certainty) = winner
wf.entries.append(winner[0])

with open('debian/watch', 'w') as f:
    wf.dump(f)

report_result(
    "Add debian/watch file, using %s." % site,
    certainty=certainty,
    fixed_lintian_tags=['debian-watch-file-is-missing'])
