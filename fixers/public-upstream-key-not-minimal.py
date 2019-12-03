#!/usr/bin/python3

import os
import subprocess
import shlex
import sys
import tempfile

gpg = shlex.split(os.environ.get('GPG', 'gpg'))


KEY_BLOCK_START = b'-----BEGIN PGP PUBLIC KEY BLOCK-----'
KEY_BLOCK_END = b'-----END PGP PUBLIC KEY BLOCK-----'


def run_gpg(args, keyring, homedir, stdin=None):
    argv = (
        gpg +
        ['--homedir', homedir, '--quiet', '--no-default-keyring',
         '--keyring=%s' % keyring] +
        args)
    try:
        p = subprocess.Popen(
            argv, stdout=subprocess.PIPE, stdin=subprocess.PIPE)
    except FileNotFoundError:
        # No gpg, no dice.
        sys.exit(2)
    (stdout, stderr) = p.communicate(stdin, timeout=5)
    if p.returncode != 0:
        raise Exception('gpg failed')
    return stdout


def gpg_export(options, keyring, homedir):
    return run_gpg(
        ['--export-options', ','.join(options), '--export', '--armor'],
        keyring=keyring, homedir=homedir)


def minimize_key_block(key):
    with tempfile.TemporaryDirectory() as td:
        keyring = os.path.join(td, 'keyring.pgp')
        homedir = os.path.join(td, 'home')
        os.mkdir(homedir, 0o700)
        run_gpg(['--import'], keyring=keyring, homedir=homedir, stdin=key)
        minimal = gpg_export(
            ['export-minimal'], keyring=keyring, homedir=homedir)
        full = gpg_export([], keyring=keyring, homedir=homedir)
        if minimal == full:
            return key
        else:
            return minimal


for p in [
        'debian/upstream/signing-key.asc',
        'debian/upstream/signing-key.pgp',
        'debian/upstream-signing-key.pgp']:
    if os.path.exists(p):
        out = []
        key = None
        with open(p, 'rb') as f:
            for line in f:
                if line.strip() == KEY_BLOCK_START:
                    key = [line]
                elif line.strip() == KEY_BLOCK_END:
                    key.append(line)
                    key = minimize_key_block(b''.join(key)).splitlines(True)
                    out.extend(key)
                    key = None
                elif key is not None:
                    key.append(line)
                else:
                    out.append(line)
            if key:
                raise Exception('Key block without end')
        with open(p, 'wb') as f:
            f.writelines(out)


print("Re-export upstream signing key without extra signatures.")
print("Fixed-Lintian-Tags: public-upstream-key-not-minimal")
