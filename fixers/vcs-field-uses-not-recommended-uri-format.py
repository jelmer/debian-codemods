#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag

FIXABLE_HOSTS = [
    'gitlab.com', 'github.com', 'salsa.debian.org',
    'gitorious.org']


with ControlEditor() as updater:
    vcs_git = updater.source.get("Vcs-Git")
    if vcs_git and ':' in vcs_git:
        (netloc, path) = vcs_git.split(':', 1)
        if netloc.startswith('git@'):
            netloc = netloc[4:]
        if netloc in FIXABLE_HOSTS:
            fixed_lintian_tag(
                updater.source, 'vcs-field-uses-not-recommended-uri-format',
                info='vcs-git %s' % updater.source['Vcs-Git'])
            updater.source["Vcs-Git"] = 'https://%s/%s' % (netloc, path)


report_result("Use recommended URI format in Vcs header.")
