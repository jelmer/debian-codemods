#!/bin/sh
sed -i '/^Standards-Version:/IcStandards-Version: 4.3.0' debian/control
echo 'Update standards version, no changes needed.'
echo 'Certainty: certain'
echo 'Fixed-Lintian-Tags: out-of-date-standards-version'
