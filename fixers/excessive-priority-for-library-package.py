#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result

changed = []


with ControlEditor() as updater:
    default_priority = updater.source.get('Priority')

    for binary in updater.binaries:
        if binary.get("Section") != 'libs':
            continue
        priority = binary.get('Priority', default_priority)
        if priority in ("required", "important", "standard"):
            binary['Priority'] = 'optional'
            changed.append(binary['Package'])


report_result(
    'Set priority for library package%s %s to optional.' % (
      's' if len(changed) > 1 else '', ', '.join(changed)),
    fixed_lintian_tags=['excessive-priority-for-library-package'])
