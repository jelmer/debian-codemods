#!/usr/bin/python3

from debmutate.control import ControlEditor, ensure_some_version
from lintian_brush.fixer import (
    report_result,
    LintianIssue,
    )
import re
from typing import List

resolution = ""


for name in ['configure.ac', 'configure.in']:
    oldlines: List[bytes] = []
    newlines: List[bytes] = []

    try:
        with open(name, 'rb') as f:
            for lineno, line in enumerate(f, 1):
                m = re.fullmatch(
                    b'\\s*AC_PATH_PROG\\s*'
                    b'\\(\\s*(\\[)?(?P<variable>[A-Z_]+)(\\])?\\s*'
                    b',\\s*(\\[)?pkg-config(\\])?\\s*'
                    b'(,\\s*(\\[)?(?P<default>.*)(\\])?\\s*)?\\)\n', line)
                if m:
                    issue = LintianIssue(
                        'source',
                        'autotools-pkg-config-macro-not-cross-compilation-'
                        'safe', info='%s (line %d)' % (name, lineno))
                    # TODO(jelmer): Check overrides
                    issue.report_fixed()
                    if (m.group('variable') == b'PKG_CONFIG' and
                            not m.group('default')):
                        newlines.append(b'PKG_PROG_PKG_CONFIG\n')
                        resolution = (
                            "This patch changes it to use "
                            "PKG_PROG_PKG_CONFIG macro from pkg.m4.")
                        # Build-Depend on pkg-config for pkg.m4
                        with ControlEditor() as control:
                            control.source['Build-Depends'] = (
                                ensure_some_version(
                                    control.source.get('Build-Depends', ''),
                                    'pkg-config'))
                    else:
                        newlines.append(
                            line.replace(b'AC_PATH_PROG', b'AC_PATH_TOOL'))
                        resolution = (
                            "This patch changes it to use AC_PATH_TOOL.")
                    continue

                newlines.append(line)
    except FileNotFoundError:
        continue
    if oldlines != newlines:
        with open(name, 'wb') as f:
            f.writelines(newlines)


report_result(
    """Use cross-build compatible macro for finding pkg-config.

The package uses AC_PATH_PROG to discover the location of pkg-config(1). This
macro fails to select the correct version to support cross-compilation.

%s

Refer to https://bugs.debian.org/884798 for details.
""" % resolution,
    patch_name='ac-path-pkgconfig')
