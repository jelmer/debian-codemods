#!/usr/bin/python3


import os
import sys
from typing import List, Optional
from urllib.error import HTTPError

try:
    from gpg import Context
except ImportError:
    sys.exit(2)

from debmutate.watch import (
    WatchEditor,
)

from lintian_brush.fixer import (
    LintianIssue,
    diligence,
    net_access_allowed,
    report_result,
    source_package_name,
    warn,
)
from lintian_brush.gpg import fetch_keys
from lintian_brush.watch import (
    KeyRetrievalFailed,
    probe_signature,
)

NUM_KEYS_TO_CHECK = 5
RELEASES_TO_INSPECT = 5


if not os.path.exists("debian/watch"):
    sys.exit(0)

if not net_access_allowed():
    sys.exit(2)

has_keys = False

for path in [
    "debian/upstream/signing-key.asc",
    "debian/upstream/signing-key.pgp",
]:
    if os.path.exists(path):
        has_keys = True


description = None

with WatchEditor() as editor:
    if not editor.watch_file:
        sys.exit(0)
    wf = editor.watch_file
    c = Context(armor=True)
    needed_keys = set()
    sigs_valid = []
    used_mangles: List[Optional[str]] = []
    for entry in wf.entries:  # noqa: C901
        try:
            pgpsigurlmangle = entry.get_option("pgpsigurlmangle")
        except KeyError:
            pgpsigurlmangle = None
        if pgpsigurlmangle and has_keys:
            continue
        try:
            pgpmode = entry.get_option("pgpmode")
        except KeyError:
            pgpmode = "default"
        else:
            if diligence() == 0:
                continue
        if pgpmode in ("gittag", "previous", "next", "self"):
            sys.exit(2)
        try:
            releases = list(
                sorted(entry.discover(source_package_name()), reverse=True)
            )
        except HTTPError as e:
            warn(f"HTTP error accessing discovery URL {e.geturl()}: {e}.")
            sys.exit(0)
        for r in releases[:RELEASES_TO_INSPECT]:
            try:
                sig_info = probe_signature(
                    r, pgpsigurlmangle=pgpsigurlmangle, gpg_context=c
                )
            except KeyRetrievalFailed:
                sys.exit(2)
            if sig_info is not None:
                sigs_valid.append(sig_info.is_valid)
                used_mangles.append(sig_info.mangle)
                needed_keys.update(sig_info.keys)
            else:
                used_mangles.append(None)
        if not all(sigs_valid[:NUM_KEYS_TO_CHECK]):
            sys.exit(0)
        found_common_mangles = set(used_mangles[:5])
        active_common_mangles = {x for x in found_common_mangles if x}
        if pgpsigurlmangle is None and active_common_mangles:
            issue = LintianIssue(
                "source", "debian-watch-does-not-check-openpgp-signature", ()
            )
            if issue.should_fix():
                # If only a single mangle is used for all releases
                # that have signatures, set that.
                if len(active_common_mangles) == 1:
                    new_mangle = active_common_mangles.pop()
                    entry.set_option("pgpsigurlmangle", new_mangle)
                # If all releases are signed, mandate it.
                if len(found_common_mangles) == 1:
                    entry.set_option("pgpmode", "mangle")
                    description = "Check upstream PGP signatures."
                else:
                    # Otherwise, fall back to auto.
                    entry.set_option("pgpmode", "auto")
                    description = (
                        "Opportunistically check upstream PGP signatures."
                    )
                issue.report_fixed()
        if not has_keys and needed_keys:
            issue = LintianIssue(
                "source", "debian-watch-file-pubkey-file-is-missing", ()
            )
            if issue.should_fix():
                if not os.path.isdir("debian/upstream"):
                    os.mkdir("debian/upstream")
                with open("debian/upstream/signing-key.asc", "wb") as f:
                    missing_keys = []
                    for fpr in needed_keys:
                        key = c.key_export_minimal(fpr)
                        if not key:
                            missing_keys.append(fpr)
                        f.write(key)
                    if missing_keys:
                        fetch_keys(missing_keys, home_dir=c.home_dir)
                        for fpr in missing_keys:
                            key = c.key_export_minimal(fpr)
                            if not key:
                                warn(f"Unable to export key {fpr}")
                                sys.exit(0)
                            f.write(key)

                issue.report_fixed()
                if description is None:
                    description = "Add upstream signing keys (%s)." % (
                        ", ".join(missing_keys)
                    )


if description:
    report_result(description)
