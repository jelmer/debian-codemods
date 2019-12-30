#!/bin/sh

sed -i 's/\r//g' debian/*.desktop
echo "Remove CRs from desktop files."
echo "Fixed-Lintian-Tags: desktop-entry-file-has-crs"
