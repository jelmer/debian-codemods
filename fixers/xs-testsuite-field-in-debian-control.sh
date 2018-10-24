#!/bin/sh
perl -p -i -e 's/XS-Testsuite: autopkgtest\n//' debian/control
echo "Remove unnecessary XS-Testsuite field in debian/control."
echo "Fixed-Lintian-Tags: xs-testsuite-field-in-debian-control"
