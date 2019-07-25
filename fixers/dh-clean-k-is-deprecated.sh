#!/bin/sh
perl -p -i -e 's/^	dh_clean -k$/	dh_prep/' debian/rules
echo "debian/rules: Use dh_prep rather than \"dh_clean -k\"."
echo "Fixed-Lintian-Tags: dh-clean-k-is-deprecated"
