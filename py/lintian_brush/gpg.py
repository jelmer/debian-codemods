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

KEY_BLOCK_START = b"-----BEGIN PGP PUBLIC KEY BLOCK-----"
KEY_BLOCK_END = b"-----END PGP PUBLIC KEY BLOCK-----"


class GpgMissing(Exception):
    """gpg command missing."""


class GpgFailed(Exception):
    """gpg command failed."""


def fetch_keys(keys, home_dir):
    import subprocess

    env = dict(os.environ)
    if home_dir:
        env["GNUPGHOME"] = home_dir
    try:
        subprocess.check_call(["gpg", "--recv-keys"] + keys, env=env)
    except FileNotFoundError as exc:
        # No gpg, no dice.
        raise GpgMissing() from exc
    except subprocess.CalledProcessError:
        return False
    return True
