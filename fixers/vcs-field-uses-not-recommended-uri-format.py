#!/usr/bin/python3

from lintian_brush.control import update_control

FIXABLE_HOSTS = [
    'gitlab.com', 'github.com', 'salsa.debian.org',
    'gitorious.org']


def use_recommended_uri_format(control):
    if "Vcs-Git" in control:
        vcs_git = control["Vcs-Git"]
        if ':' not in vcs_git:
            return
        (netloc, path) = vcs_git.split(':', 1)
        if netloc.startswith('git@'):
            netloc = netloc[4:]
        if netloc in FIXABLE_HOSTS:
            control["Vcs-Git"] = 'https://%s/%s' % (netloc, path)


update_control(source_package_cb=use_recommended_uri_format)

print("Use recommended URI format in Vcs header.")
print("Fixed-Lintian-Tags: vcs-field-uses-not-recommended-uri-format")
