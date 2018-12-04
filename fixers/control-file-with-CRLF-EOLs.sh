#!/bin/sh
which dos2unix >/dev/null || exit 2
dos2unix -q debian/control*
echo "Format control file with unix-style line endings."
echo "Fixed-Lintian-Tags: control-file-with-CRLF-EOLs"
