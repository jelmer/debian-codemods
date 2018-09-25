#!/bin/sh
perl -p -i -e 's/ --with[ =]quilt( .+|)$/$1/m' debian/rules
perl -p -i -e 's/ --with[ =]quilt,/ --with=/' debian/rules
perl -p -i -e 's/ --with[ =]([^ ]),quilt([ ,])/ --with=$1$2/' debian/rules
echo "Don't specify --with=quilt, since package uses '3.0 (quilt)' source format."
