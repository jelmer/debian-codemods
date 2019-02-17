#!/bin/sh

perl -p -e 's/(closes) #([0-9]+)/\1: #\2/gi' -i debian/changelog

echo "Add missing colon in closes line."
echo "Fixed-Lintian-Tags: possible-missing-colon-in-closes"
