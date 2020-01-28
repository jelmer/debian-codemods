#!/usr/bin/python3

import os
import sys

from lintian_brush.lintian_overrides import remove_unused

if int(os.environ.get('DILIGENCE', '0')) < 1:
    # Removing unused overrides requires pro-actively contacting UDD.
    sys.exit(0)

removed = remove_unused()

print('Remove %d unused lintian overrides.' % len(removed))
print('')
for override in removed:
    print('* %s' % override.tag)
print('Fixed-Lintian-Tags: unused-override')
print('Certainty: certain')
