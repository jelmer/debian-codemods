#!/usr/bin/python3

import os

from lintian_brush.fixer import fixed_lintian_tag, report_result

removed = []

for name in os.listdir('debian'):
    if name.endswith('.linda-overrides'):
        os.unlink(os.path.join('debian', name))
        removed.append(name)
        fixed_lintian_tag(
            'source', 'package-contains-linda-override',
            'usr/share/linda/overrides/%s' % name[:-len('.linda-overrides')])

report_result('Remove obsolete linda overrides: ' + ', '.join(removed))
