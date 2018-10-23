#!/bin/sh
perl -p -i -e 's/XS-Vcs-(.*): (.*)\n/Vcs-\1: \2/' debian/control
echo "Remove unnecessary XS- prefix for Vcs- fields in debian/control."
echo "Fixed-Lintian-Tags: xs-vcs-field-in-debian-control"
