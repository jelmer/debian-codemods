#!/usr/bin/python3
import os
import subprocess
import sys

if os.path.exists('debian/watch'):
    # Nothing to do here..
    sys.exit(0)

watch_contents = None
site = None
if os.path.exists('setup.py'):
    (project, version) = subprocess.check_output(
        ['python3', 'setup.py', '--name', '--version']).splitlines()
    # TODO(jelmer): verify that <name>-<version> appears on
    # https://pypi.python.org/simple/<name>
    # TODO(jelmer): download watch file from
    # http://pypi.debian.net/<project>/watch
    watch_contents = r"""version=3
opts=uversionmangle=s/(rc|a|b|c)/~$1/,pgpsigurlmangle=s/$/.asc/ \
https://pypi.debian.net/%(project)s/%(project)s-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))
""" % {"project": project.decode()}
    site = "pypi"

if not watch_contents:
    sys.exit(0)

with open('debian/watch', 'w') as f:
    f.write(watch_contents)

print("Add debian/watch file, using %s." % site)
print("Certainty: possible")
print("Fixed-Lintian-Tags: debian-watch-file-is-missing")
