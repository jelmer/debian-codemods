#!/bin/sh
sed -i 's/\r//g' debian/copyright
echo "Remove CRs from copyright file."
echo "Fixed-Lintian-Tags: copyright-has-crs"
