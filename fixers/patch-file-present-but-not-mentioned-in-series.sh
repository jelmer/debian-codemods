#!/bin/sh -eu
test -r debian/patches/series || exit 0
cd debian/patches

for f in * ; do
	test "${f}" != series || continue
	grep -q -- "${f}" series || rm "${f}"
done
echo "Remove patches missing from debian/patches/series."
echo "Fixed-Lintian-Tags: patch-file-present-but-not-mentioned-in-series"
