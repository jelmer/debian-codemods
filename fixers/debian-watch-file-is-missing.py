#!/usr/bin/python3
import os
import subprocess
import sys

from lintian_brush.watch import WatchFile, Watch


if os.path.exists('debian/watch'):
    # Nothing to do here..
    sys.exit(0)

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
    # TODO(jelmer): verify that <name>-<version> appears on
    # https://pypi.python.org/simple/<name>
    # TODO(jelmer): download watch file from
    # http://pypi.debian.net/<project>/watch
    url = (r'https://pypi.debian.net/%(project)s/%(project)'
           r's-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))' % {
            'project': project.decode()})
    w = Watch(url, opts=[
        'uversionmangle=s/(rc|a|b|c)/~$1/', 'pgpsigurlmangle=s/$/.asc/'])
    wf.entries.append(w)
    site = "pypi"


def guess_github_watch_entry(parsed_url, upstream_version):
    from breezy.branch import Branch
    import re
    if os.environ.get('NET_ACCESS', 'allow') != 'allow':
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
print("Certainty: possible")
print("Fixed-Lintian-Tags: debian-watch-file-is-missing")
