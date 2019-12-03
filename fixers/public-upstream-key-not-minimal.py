#!/usr/bin/python3

import os
import subprocess
import shlex
import sys
import tempfile

gpg = shlex.split(os.environ.get('GPG', 'gpg'))


def run_gpg(args, keyring, homedir):
    argv = (
        gpg +
        ['--homedir', homedir, '--quiet', '--no-default-keyring',
         '--keyring=%s' % keyring] +
        args)
    try:
        return subprocess.check_output(argv)
    except FileNotFoundError:
        # No gpg, no dice.
        sys.exit(2)


def gpg_export(options, keyring, homedir):
    return run_gpg(
        ['--export-options', ','.join(options), '--export', '--armor'],
        keyring=keyring, homedir=homedir)


def gpg_export_minimal(target, keyring, homedir):
    minimal = gpg_export(['export-minimal'], keyring=keyring, homedir=homedir)
    full = gpg_export([], keyring=keyring, homedir=homedir)
    if minimal != full:
        with open(target, 'wb') as f:
            f.write(minimal)


for p in [
        'debian/upstream/signing-key.asc',
        'debian/upstream/signing-key.pgp',
        'debian/upstream-signing-key.pgp']:
    if os.path.exists(p):
        with tempfile.TemporaryDirectory() as td:
            keyring = os.path.join(td, 'keyring.pgp')
            homedir = os.path.join(td, 'home')
            os.mkdir(homedir)
            run_gpg(['--import', p], keyring=keyring, homedir=homedir)
            gpg_export_minimal(p, keyring=keyring, homedir=homedir)


print("Re-export upstream signing key without extra signatures.")
print("Fixed-Lintian-Tags: public-upstream-key-not-minimal")
