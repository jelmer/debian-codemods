#!/bin/bash
if [[ -f debian/upstream ]]; then
	mv debian/upstream debian/upstream-metadata.yaml
fi
if test -f debian/upstream-metadata -o -f debian/upstream-metadata.yaml; then
	mkdir -p debian/upstream
	test -f debian/upstream-metadata && mv debian/upstream-metadata debian/upstream/metadata
	test -f debian/upstream-metadata.yaml && mv debian/upstream-metadata.yaml debian/upstream/metadata
fi
echo "Move upstream metadata to debian/upstream/metadata."
echo "Fixed-Lintian-Tags: debian-upstream-obsolete-path"
