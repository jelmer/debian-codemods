#!/usr/bin/python
# Copyright (C) 2020 Jelmer Vernooij
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

import os
import shlex
import subprocess
import tempfile

gpg = shlex.split(os.environ.get("GPG", "gpg"))

KEY_BLOCK_START = b"-----BEGIN PGP PUBLIC KEY BLOCK-----"
KEY_BLOCK_END = b"-----END PGP PUBLIC KEY BLOCK-----"


class GpgMissing(Exception):
    """gpg command missing"""


class GpgFailed(Exception):
    """gpg command failed"""


def gpg_import_export(import_options, export_options, stdin):
    argv = gpg + [
        "--armor",
        "--quiet",
        "--no-default-keyring",
        "--export-options",
        ",".join(export_options),
        "--import-options",
        ",".join(["import-export"] + import_options),
        "--output",
        "-",
        "--import",
        "-",
    ]
    with tempfile.TemporaryDirectory() as td:
        try:
            p = subprocess.Popen(
                argv, stdout=subprocess.PIPE, stdin=subprocess.PIPE,
                env={'GNUPGHOME': td})
        except FileNotFoundError:
            # No gpg, no dice.
            raise GpgMissing()
        (stdout, stderr) = p.communicate(stdin, timeout=5)
        if p.returncode != 0:
            raise GpgFailed(stderr)
        return stdout
