#!/bin/sh
if [ -f debian/source.lintian-overrides ]; then
    if [ ! -d debian/source ]; then
        mkdir debian/source
    fi
    mv debian/source.lintian-overrides debian/source/lintian-overrides
fi
echo "Move source package lintian overrides to debian/source."
echo "Fixed-Lintian-Tags: old-source-override-location"
