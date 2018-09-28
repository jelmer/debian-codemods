#!/bin/sh
# Not yet enabled; this should only be done if it doesn't add additional lintian warnings about copyright file.
perl -p -i -e 's/^(Format|Format-Specification): .*/Format: https:\/\/www.debian.org\/doc\/packaging-manuals\/copyright-format\/1.0\//' debian/copyright
echo "Use versioned copyright format URI."
