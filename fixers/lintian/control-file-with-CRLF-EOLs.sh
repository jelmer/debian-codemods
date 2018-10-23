#!/bin/sh
dos2unix debian/control*
echo "Format control file with unix-style line endings."
echo "Fixed-Lintian-Tags: control-file-with-CRLF-EOLs"
