#!/bin/sh
perl -p -i -e 's/^Testsuite: autopkgtest\n//' debian/control*
echo "Remove unnecessary 'Testsuite: autopkgtest' header."
echo "Fixed-Lintian-Tags: unnecessary-testsuite-autopkgtest-field"
