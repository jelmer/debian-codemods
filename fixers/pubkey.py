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

from debmutate.watch import WatchEditor, apply_subst_expr

from lintian_brush.fixer import (
    source_package_name,
    LintianIssue,
    report_result,
    warn,
    )

COMMON_MANGLES = ['s/$/.asc/']


if not os.path.exists('debian/watch'):
    sys.exit(0)

has_keys = False

for path in ['debian/upstream/signing-key.asc',
             'debian/upstream/signing-key.pgp']:
    if os.path.exists(path):
        has_keys = True


def sig_valid(sig):
    return sig.status == 0


with WatchEditor() as editor:
    if not editor.watch_file:
        sys.exit(0)
    wf = editor.watch_file
    c = Context(armor=True)
    needed_keys = set()
    sigs_valid = []
    used_mangles: List[Optional[str]] = []
    for entry in wf.entries:
        if entry.has_option('pgpsigurlmangle') and has_keys:
            continue
        for r in sorted(entry.discover(source_package_name()), reverse=True):
            if r.pgpsigurl:
                pgpsigurls = [
                    (entry.get_option('pgpsigurlmangle'), r.pgpsigurl)]
            else:
                pgpsigurls = [
                    (mangle, apply_subst_expr(mangle, r.url))
                    for mangle in COMMON_MANGLES]
            for mangle, pgpsigurl in pgpsigurls:
                # Try and download signatures from some predictable locations.
                try:
                    resp = urlopen(pgpsigurl)
                except HTTPError:
                    continue
                sig = resp.read()
                actual = urlopen(r.url).read()
                gr = c.verify(actual, sig)[1]
                is_valid = True
                for sig in gr.signatures:
                    # TODO(jelmer): Check validity
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
        if not all(sigs_valid[:5]):
            sys.exit(0)
        common_mangles = set(used_mangles[:5])
        if len(common_mangles) == 1:
            new_mangle = common_mangles.pop()
            issue = LintianIssue(
                'source', 'debian-watch-does-not-check-gpg-signature', ())
            if new_mangle is not None and issue.should_fix():
                entry.set_option('pgpsigurlmangle', new_mangle)
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
                        import subprocess
                        subprocess.check_call(
                            ['gpg', '--recv-keys'] + missing_keys)
                        for fpr in missing_keys:
                            key = c.key_export_minimal(fpr)
                            if not key:
                                warn('Unable to export key %s' % (fpr, ))
                                sys.exit(0)
                            f.write(key)

                issue.report_fixed()


report_result('Check upstream signatures.')
