#!/bin/sh

perl -pe 'BEGIN{undef $/;} s/(Local variables:.*?)
add-log-mailing-address: .*
(End:)
/
\1
\2
/smg' -i debian/changelog

perl -pe 'BEGIN{undef $/;} s/([\n]*Local variables:.*?)
mode: debian-changelog
(End:[\n]+)/
/smg' -i debian/changelog

echo "Drop no longer supported add-log-mailing-address setting from debian/changelog."
echo "Fixed-Lintian-Tags: debian-changelog-file-contains-obsolete-user-emacs-setting"
