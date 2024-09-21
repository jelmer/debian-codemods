#!/bin/sh
perl -p -i -e 's/XS-Testsuite: autopkgtest\n//' debian/control*
perl -p -i -e 's/XS-Testsuite: (.*)\n/Testsuite: \1\n/' debian/control*
echo "Remove unnecessary XS-Testsuite field in debian/control."
echo "Fixed-Lintian-Tags: adopted-extended-field"
