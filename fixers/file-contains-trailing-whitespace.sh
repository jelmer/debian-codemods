#!/bin/sh
sed -i -e 's@[[:space:]]*$@@g' debian/control* debian/changelog
sed -i -e 's@[ ]*$@@g' debian/rules
sed -i -e :a -e '/^\n*$/{$d;N;};/\n$/ba' debian/rules debian/changelog debian/control*
echo "Trim trailing whitespace."
echo "Fixed-Lintian-Tags: file-contains-trailing-whitespace"
