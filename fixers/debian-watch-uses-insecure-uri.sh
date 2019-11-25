#!/bin/sh
test -r debian/watch        || exit 0
grep 'http://' debian/watch>/dev/null || exit 0

# We hardcode the replacements for some sites, since these testsuite uses these.
# The method below (involving uscan) doesn't work from e.g. sbuild hosts.
perl -p -i -e 's/http:\/\/code.launchpad.net\//https:\/\/code.launchpad.net\//' debian/watch
perl -p -i -e 's/http:\/\/launchpad.net\//https:\/\/launchpad.net\//' debian/watch
perl -p -i -e 's/http:\/\/ftp.gnu.org\//https:\/\/ftp.gnu.org\//' debian/watch

echo "Use secure URI in debian/watch."
echo "Fixed-Lintian-Tags: debian-watch-uses-insecure-uri"

grep 'http://' debian/watch >/dev/null|| exit 0

before=$(mktemp)
after=$(mktemp)
uscan --dehs > "${before}" 2>&1
sed -i.bak s,http://,https://,g debian/watch
uscan --dehs > "${after}" 2>&1
# uscan creates backup files.
rm -f debian/upstream/signing-key.pgp.backup debian/upstream-signing-key.pgp.backup

# Make sure that reports are same up to http/https substitution in URL.
sed -i s,http://,https://,g "${before}" "${after}"
if cmp -s "${before}" "${after}" ; then
       rm -f debian/watch.bak
else
       mv debian/watch.bak debian/watch
fi
rm -f "${before}" "${after}"
