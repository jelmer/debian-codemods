#!/usr/bin/python3

import os
import sys
from typing import List, Optional

from lintian_brush.fixer import report_result, fixed_lintian_tag
from lintian_brush.gpg import (
    gpg_import_export,
    GpgMissing,
    KEY_BLOCK_START,
    KEY_BLOCK_END,
    )


def minimize_key_block(key):
    minimal = gpg_import_export(
        ['import-minimal', 'import-clean', 'self-sigs-only', 'repair-keys'],
        ['export-clean'], key)
    full = gpg_import_export(
        ['no-import-minimal', 'no-import-clean', 'no-self-sigs-only',
         'no-repair-keys', 'import-restore'], [], key)
    if minimal == full:
        return key
    else:
        return minimal


for p in [
        'debian/upstream/signing-key.asc',
        'debian/upstream/signing-key.pgp',
        'debian/upstream-signing-key.pgp']:
    if os.path.exists(p):
        outlines: List[bytes] = []
        key: Optional[List[bytes]] = None
        with open(p, 'rb') as f:
            inlines = list(f.readlines())
        for line in inlines:
            if line.strip() == KEY_BLOCK_START:
                key = [line]
            elif line.strip() == KEY_BLOCK_END and key is not None:
                key.append(line)
                try:
                    outlines.extend(minimize_key_block(
                        b''.join(key)).splitlines(True))
                except GpgMissing:
                    sys.exit(2)
                key = None
            elif key is not None:
                key.append(line)
            else:
                outlines.append(line)
        if key:
            raise Exception('Key block without end')
        if inlines != outlines:
            fixed_lintian_tag(
                'source', 'public-upstream-key-not-minimal')
            with open(p, 'wb') as g:
                g.writelines(outlines)


report_result("Re-export upstream signing key without extra signatures.")
