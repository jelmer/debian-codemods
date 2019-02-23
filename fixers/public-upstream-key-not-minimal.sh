#!/bin/sh

which gpg >/dev/null || exit 2

TEMP_KEYRING=$(mktemp)

run_gpg() {
    gpg --quiet --no-default-keyring --keyring "${TEMP_KEYRING}" "$@"
}

gpg_export() {
    run_gpg --export-options "$1" --export --armor
}

gpg_export_minimal() {
    target="$1"
    gpg_export export-minimal > "${target}.minimal"
    gpg_export "" > "${target}.full"
    if ! diff "${target}.minimal" "${target}.full" > /dev/null; then
        mv "${target}.minimal" "${target}"
    fi
    rm -f "${target}.full" "${target}.minimal"
}

if [ -f debian/upstream/signing-key.asc ]; then
    run_gpg --import debian/upstream/signing-key.asc
    gpg_export_minimal debian/upstream/signing-key.asc
fi

for p in debian/upstream/signing-key.pgp debian/upstream-signing-key.pgp
do
    if [ -f "$p" ]; then
        run_gpg --import "$p"
        gpg_export_minimal "$p"
    fi
done

echo "Re-export upstream signing key without extra signatures."
echo "Fixed-Lintian-Tags: public-upstream-key-not-minimal"
