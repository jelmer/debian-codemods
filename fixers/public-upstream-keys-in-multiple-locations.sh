#!/bin/sh

GPG=${GPG:-gpg}

${GPG} --version 2>/dev/null >/dev/null || exit 2

OTHER_PATHS="debian/upstream/signing-key.pgp debian/upstream-signing-key.pgp"
MAIN_PATH=debian/upstream/signing-key.asc

if [ $(ls $OTHER_PATHS $MAIN_PATH 2>/dev/null | wc -l) -lt 2 ]; then
    exit 0
fi

TEMP_KEYRING=$(mktemp)

run_gpg() {
    ${GPG} --quiet --no-default-keyring --keyring "${TEMP_KEYRING}" "$@"
}

gpg_export() {
    run_gpg --export-options export-minimal --export "$@"
}

for p in $OTHER_PATHS $MAIN_PATH
do
    if [ -f "$p" ]; then
        run_gpg --import "$p"
    fi
done

gpg_export --armor > "debian/upstream/signing-key.asc"

for p in $OTHER_PATHS
do
    rm -f $p
done

echo "Merge upstream signing key files."
echo "Fixed-Lintian-Tags: public-upstream-keys-in-multiple-locations."
