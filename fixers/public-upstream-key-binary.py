#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import report_result
from lintian_brush.gpg import gpg_import_export, GpgMissing

try:
    with open('debian/upstream/signing-key.pgp', 'rb') as f:
        key = f.read()
except FileNotFoundError:
    sys.exit(0)

try:
    full = gpg_import_export(
        ['no-import-minimal', 'no-import-clean', 'no-self-sigs-only',
         'no-repair-keys', 'import-restore'], [], key)
except GpgMissing:
    sys.exit(2)

with open('debian/upstream/signing-key.asc', 'wb') as f:
    f.write(full)

os.remove('debian/upstream/signing-key.pgp')

report_result("Enarmor upstream signing key.")
