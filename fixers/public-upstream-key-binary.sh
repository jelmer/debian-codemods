#!/bin/sh -e

GPG=${GPG:-gpg}

if [ -f debian/upstream/signing-key.pgp ]; then
    gpg --output debian/upstream/signing-key.asc --enarmor debian/upstream/signing-key.pgp
    rm -f debian/upstream/signing-key.pgp
fi

echo "Enarmor upstream signing key."
