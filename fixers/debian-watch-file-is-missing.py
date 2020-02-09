#!/usr/bin/python3
import json
import os
import re
import subprocess
import sys
from urllib.request import urlopen, Request

from lintian_brush import USER_AGENT, DEFAULT_URLLIB_TIMEOUT
from lintian_brush.fixer import net_access_allowed
from lintian_brush.watch import WatchFile, Watch


if os.path.exists('debian/watch'):
    # Nothing to do here..
    sys.exit(0)


certainty = 'possible'

watch_contents = None
site = None
wf = WatchFile()
if os.path.exists('setup.py'):
    try:
        lines = subprocess.check_output(
            ['python3', 'setup.py', '--name', '--version']).splitlines()
    except subprocess.CalledProcessError:
        # Worth a shot..
        lines = subprocess.check_output(
            ['python2', 'setup.py', '--name', '--version']).splitlines()
    lines = [line for line in lines if not line.startswith(b'W: ')]
    (project, version) = lines
    project = project.decode()
    version = version.decode()
    current_version_filenames = None
    if net_access_allowed():
        json_url = 'https://pypi.python.org/pypi/%s/json' % project
        headers = {'User-Agent': USER_AGENT}
        response = urlopen(
            Request(json_url, headers=headers), timeout=DEFAULT_URLLIB_TIMEOUT)
        pypi_data = json.load(response)
        if version in pypi_data['releases']:
            release = pypi_data['releases'][version]
            current_version_filenames = [
                (d['filename'], d['has_sig'])
                for d in release if d['packagetype'] == 'sdist']
    filename_regex = (
        r'%(project)s-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))' % {
            'project': project})
    opts = []
    # TODO(jelmer): Set uversionmangle?
    # opts.append('uversionmangle=s/(rc|a|b|c)/~$1/')
    if current_version_filenames:
        for (fn, has_sig) in current_version_filenames:
            if re.match(filename_regex, fn):
                certainty = 'certain'
                if has_sig:
                    opts.append('pgpsigurlmangle=s/$/.asc/')
    url = (r'https://pypi.debian.net/%(project)s/%(fname_regex)s' % {
        'project': project, 'fname_regex': filename_regex})
    # TODO(jelmer): Add pgpsigurlmangle if has_sig==True
    w = Watch(url, opts=opts)
    wf.entries.append(w)
    site = "pypi"


def guess_github_watch_entry(parsed_url, upstream_version):
    from breezy.branch import Branch
    import re
    if not net_access_allowed():
        return None
    branch = Branch.open(code['Repository'])
    tags = branch.tags.get_tag_dict()
    POSSIBLE_PATTERNS = [r"v(\d\S+)", r"(\d\S+)"]
    version_pattern = None
    for name in sorted(tags, reverse=True):
        for pattern in POSSIBLE_PATTERNS:
            m = re.match(pattern, name)
            if not m:
                continue
            if m.group(1) == upstream_version:
                version_pattern = pattern
                break
        if version_pattern:
            break
    else:
        return None
    (username, project) = parsed_url.path.strip('/').split('/')
    download_url = 'https://github.com/%(user)s/%(project)s/tags' % {
        'user': username, 'project': project}
    matching_pattern = r'.*/%s\.tar\.gz' % version_pattern
    opts = []
    opts.append(
        r'filenamemangle=s/%(pattern)s/%(project)s-$1\.tar\.gz/' % {
            'pattern': matching_pattern,
            'project': project})
    # TODO(jelmer): Check for GPG
    # opts.append(
    #    r'pgpsigurlmangle='
    #    r's/archive\/%s\.tar\.gz/releases\/download\/$1\/$1\.tar\.gz\.asc/' %
    #    version_pattern)
    return Watch(download_url, matching_pattern, opts=opts)


if not wf.entries:
    try:
        with open('debian/upstream/metadata', 'r') as f:
            inp = f.read()
    except FileNotFoundError:
        pass
    else:
        import ruamel.yaml
        code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)

        from urllib.parse import urlparse
        from debian.changelog import Version

        try:
            parsed_url = urlparse(code['Repository'])
        except KeyError:
            pass
        else:
            upstream_version = Version(
                os.environ['CURRENT_VERSION']).upstream_version
            if parsed_url.hostname == 'github.com':
                w = guess_github_watch_entry(parsed_url, upstream_version)
                if w:
                    site = 'github'
                    wf.entries.append(w)


if not wf.entries:
    sys.exit(0)

with open('debian/watch', 'w') as f:
    wf.dump(f)

print("Add debian/watch file, using %s." % site)
print("Certainty: %s" % certainty)
print("Fixed-Lintian-Tags: debian-watch-file-is-missing")
