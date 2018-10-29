#!/bin/bash
if [ -f debian/patches/series ]; then
	[ -n "$(tail -c1 debian/patches/series)" ] && echo >> debian/patches/series
fi
echo "Add missing trailing newline in debian/patches/series."
echo "Fixed-Lintian-Tags: quilt-series-without-trailing-newline"
