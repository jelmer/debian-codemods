#!/bin/sh

TEMP_KEYRING=$(mktemp)

run_gpg() {
    gpg --no-default-keyring --keyring "${TEMP_KEYRING}" "$@"
}

gpg_export() {
    run_gpg --export-options export-minimal,export-clean --export "$@"
}

if [ -f debian/upstream/signing-key.asc ]; then
    run_gpg --import debian/upstream/signing-key.asc
    gpg_export --armor > debian/upstream/signing-key.asc
fi

if [ -f debian/upstream/signing-key.pgp ]; then
    run_gpg --import debian/upstream/signing-key.pgp
    gpg_export > debian/upstream/signing-key.pgp
fi

echo "Re-export upstream signing key without extra signatures."
echo "Fixed-Lintian-Tags: public-upstream-key-not-minimal"
