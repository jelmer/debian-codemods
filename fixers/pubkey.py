#!/usr/bin/python3


import os
import sys
from typing import List, Optional
from urllib.request import urlopen
from urllib.error import HTTPError

try:
    from gpg import Context
except ImportError:
    sys.exit(2)

import gpg.errors

from debmutate.watch import WatchEditor, apply_url_mangle

from lintian_brush.fixer import (
    source_package_name,
    LintianIssue,
    report_result,
    warn,
    diligence,
    )

COMMON_MANGLES = [
    's/$/.%s/' % ext for ext in ['asc', 'pgp', 'gpg', 'sig', 'sign']]
NUM_KEYS_TO_CHECK = 5
RELEASES_TO_INSPECT = 5


if not os.path.exists('debian/watch'):
    sys.exit(0)

has_keys = False

for path in ['debian/upstream/signing-key.asc',
             'debian/upstream/signing-key.pgp']:
    if os.path.exists(path):
        has_keys = True


def fetch_keys(keys):
    import subprocess
    subprocess.check_call(['gpg', '--recv-keys'] + keys)


def sig_valid(sig):
    return sig.status == 0


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
            pgpsigurlmangle = entry.get_option('pgpsigurlmangle')
        except KeyError:
            pgpsigurlmangle = None
        if pgpsigurlmangle and has_keys:
            continue
        try:
            pgpmode = entry.get_option('pgpmode')
        except KeyError:
            pgpmode = 'default'
        else:
            if diligence() == 0:
                continue
        if pgpmode in ('gittag', 'previous', 'next', 'self'):
            sys.exit(2)
        releases = list(sorted(entry.discover(source_package_name()), reverse=True))
        for r in releases[:RELEASES_TO_INSPECT]:
            if r.pgpsigurl:
                pgpsigurls = [(pgpsigurlmangle, r.pgpsigurl)]
            else:
                pgpsigurls = [
                    (mangle, apply_url_mangle(mangle, r.url))
                    for mangle in COMMON_MANGLES]
            for mangle, pgpsigurl in pgpsigurls:
                # Try and download signatures from some predictable locations.
                try:
                    resp = urlopen(pgpsigurl)
                except HTTPError:
                    continue
                sig = resp.read()
                actual = urlopen(r.url).read()
                try:
                    gr = c.verify(actual, sig)[1]
                except gpg.errors.GPGMEError as e:
                    warn('Error verifying signature %s on %s: %s' % (
                         pgpsigurl, r.url, e))
                    continue
                except gpg.errors.BadSignatures as e:
                    if str(e).endswith(': No public key'):
                        fetch_keys([s.fpr for s in e.result.signatures])
                        gr = c.verify(actual, sig)[1]
                    else:
                        raise
                signatures = gr.signatures
                is_valid = True
                for sig in signatures:
                    if not sig_valid(sig):
                        warn('Signature from %s in %s for %s not valid' % (
                             sig.fpr, pgpsigurl, r.url))
                        is_valid = False
                    else:
                        needed_keys.add(sig.fpr)
                sigs_valid.append(is_valid)
                used_mangles.append(mangle)
                break
            else:
                used_mangles.append(None)
        if not all(sigs_valid[:NUM_KEYS_TO_CHECK]):
            sys.exit(0)
        found_common_mangles = set(used_mangles[:5])
        active_common_mangles = set([x for x in found_common_mangles if x])
        if pgpsigurlmangle is None and active_common_mangles:
            issue = LintianIssue(
                'source', 'debian-watch-does-not-check-gpg-signature', ())
            if issue.should_fix():
                # If only a single mangle is used for all releases
                # that have signatures, set that.
                if len(active_common_mangles) == 1:
                    new_mangle = active_common_mangles.pop()
                    entry.set_option('pgpsigurlmangle', new_mangle)
                # If all releases are signed, mandate it.
                if len(found_common_mangles) == 1:
                    entry.set_option('pgpmode', 'mangle')
                    description = "Check upstream PGP signatures."
                else:
                    # Otherwise, fall back to auto.
                    entry.set_option('pgpmode', 'auto')
                    description = "Opportunistically check upstream PGP signatures."
                issue.report_fixed()
        if not has_keys and needed_keys:
            issue = LintianIssue(
                'source', 'debian-watch-file-pubkey-file-is-missing', ())
            if issue.should_fix():
                if not os.path.isdir('debian/upstream'):
                    os.mkdir('debian/upstream')
                with open('debian/upstream/signing-key.asc', 'wb') as f:
                    missing_keys = []
                    for fpr in needed_keys:
                        key = c.key_export_minimal(fpr)
                        if not key:
                            missing_keys.append(fpr)
                        f.write(key)
                    if missing_keys:
                        fetch_keys(missing_keys)
                        for fpr in missing_keys:
                            key = c.key_export_minimal(fpr)
                            if not key:
                                warn('Unable to export key %s' % (fpr, ))
                                sys.exit(0)
                            f.write(key)

                issue.report_fixed()
                if description is None:
                    description = "Add upstream signing keys (%s)." % (
                        ', '.join(missing_keys))


if description:
    report_result(description)
