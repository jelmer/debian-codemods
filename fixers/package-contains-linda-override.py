#!/usr/bin/python3

import os
removed = []

for name in os.listdir('debian'):
    if name.endswith('.linda-overrides'):
        os.unlink(os.path.join('debian', name))
        removed.append(name)

print('Remove obsolete linda overrides: ' + ', '.join(removed))
print('Fixed-Lintian-Tags: package-contains-linda-override')
