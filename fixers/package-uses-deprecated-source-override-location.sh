#!/bin/sh
if [ ! -f debian/source.lintian-overrides ]; then
    echo "File missing" >&2
    exit 1
fi
if [ ! -d debian/source ]; then
    mkdir debian/source
fi
mv debian/source.lintian-overrides debian/source/lintian-overrides
echo "Move source package lintian overrides to debian/source."
echo "Fixed-Lintian-Tags: package-uses-deprecated-source-override-location"
