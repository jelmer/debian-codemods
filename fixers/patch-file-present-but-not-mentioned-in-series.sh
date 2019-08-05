#!/bin/sh -eu
test -r debian/patches/series || exit 0
cd debian/patches

for f in * ; do
	# Don't delete the series file
	test "${f}" != series || continue
	# Ignore everything that is not a regular file
	test -f "${f}" || continue
	# Ignore any README files
	test "${f}" != README || continue
	# Ignore everything that is listed in series
	grep -q -- "${f}" series || rm "${f}"
done
echo "Remove patches missing from debian/patches/series."
echo "Fixed-Lintian-Tags: patch-file-present-but-not-mentioned-in-series"
