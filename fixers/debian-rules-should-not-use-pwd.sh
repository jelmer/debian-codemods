#!/bin/sh
perl -p -i -e 's/\$\(PWD\)/\$\(CURDIR\)/' debian/rules
echo "debian/rules: Avoid using \$(PWD) variable."
echo "Fixed-Lintian-Tags: debian-rules-should-not-use-pwd"
