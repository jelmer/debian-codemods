#!/usr/bin/python3

from debmutate.control import ensure_some_version, get_relation
from lintian_brush.fixer import control, report_result, LintianIssue
import sys
try:
    from tomlkit import load
except ModuleNotFoundError:
    sys.exit(2)

try:
    with open('pyproject.toml') as f:
        toml = load(f)
except FileNotFoundError:
    sys.exit(0)


build_backend = toml.get('build-system', {}).get('build-backend')

# See /usr/share/lintian/lib/Lintian/Check/Languages/Python.pm
PREREQUISITE_MAP = {
    'poetry.core.masonry.api': 'python3-poetry-core',
    'flit_core.buildapi': 'flit',
    'setuptools.build_meta': 'python3-setuptools'
}


try:
    prerequisite = PREREQUISITE_MAP[build_backend]
except KeyError:
    sys.exit(2)

with control:
    for field in ['Build-Depends', 'Build-Depends-Indep',
                  'Build-Depends-Arch']:
        try:
            if get_relation(control.source.get(field, ''), prerequisite):
                sys.exit(0)
        except KeyError:
            pass
    # TOOD(jelmer): Add file:lineno; requires
    # https://github.com/sdispater/tomlkit/issues/55
    issue = LintianIssue(
        control.source,
        'missing-prerequisite-for-pyproject-backend',
        info='%s (does not satisfy %s)' % (
            build_backend, prerequisite))
    if issue.should_fix():
        control.source['Build-Depends'] = ensure_some_version(
            control.source.get('Build-Depends', ''), prerequisite)
        issue.report_fixed()

report_result(
    'Add missing build-dependency on %s.\n\n'
    'This is necessary for build-backend %s in pyproject.toml' % (
        prerequisite,
        build_backend))
