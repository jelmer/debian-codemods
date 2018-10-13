#!/bin/sh
# TODO(jelmer): Find the latest version of debian-policy to use here, instead of hardcoding 4.2.1
perl -p -i -e 's/^Standards-Version: .*/Standards-Version: 4.2.1/' debian/control
echo "Use most recent version in Standards-Version field rather than non-existant future version."
