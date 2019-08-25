#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij <jelmer@debian.org>
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

"""Functions for working with upstream metadata."""

import json
import os
import re
import subprocess
import tempfile
from urllib.parse import urlparse, urlunparse
from warnings import warn
from lintian_brush import USER_AGENT
from lintian_brush.vcs import (
    sanitize_url as sanitize_vcs_url,
    )
from lintian_brush.watch import parse_watch_file
from urllib.request import urlopen, Request


KNOWN_HOSTING_SITES = [
    'code.launchpad.net', 'github.com', 'launchpad.net']
KNOWN_GITLAB_SITES = ['gitlab.com', 'salsa.debian.org']


def get_sf_metadata(project):
    headers = {'User-Agent': USER_AGENT}
    http_url = 'https://sourceforge.net/rest/p/%s' % project
    http_contents = urlopen(Request(http_url, headers=headers)).read()
    return json.loads(http_contents)


def guess_repo_from_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        if parsed_url.path.strip('/').count('/') < 1:
            return None
        return ('https://github.com' +
                '/'.join(parsed_url.path.split('/')[:3]))
    if parsed_url.netloc == 'launchpad.net':
        return 'https://code.launchpad.net/%s' % (
            parsed_url.path.strip('/').split('/')[0])
    if parsed_url.netloc in KNOWN_GITLAB_SITES:
        if parsed_url.path.strip('/').count('/') < 1:
            return None
        parts = parsed_url.path.split('/')
        if 'tags' in parts:
            parts = parts[:parts.index('tags')]
        return urlunparse(
            parsed_url._replace(path='/'.join(parts), query=''))
    if parsed_url.netloc in KNOWN_HOSTING_SITES:
        return url
    return None


def browse_url_from_repo_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path = '/'.join(parsed_url.path.split('/')[:3])
        if path.endswith('.git'):
            path = path[:-4]
        return ('https://github.com' + path)
    return None


def read_python_pkg_info(path):
    """Get the metadata from a python setup.py file."""
    from pkginfo.utils import get_metadata
    return get_metadata(path)


def get_python_pkg_info(path, trust_package=False):
    pkg_info = read_python_pkg_info(path)
    if pkg_info.name:
        return pkg_info
    if not trust_package:
        return pkg_info
    filename = os.path.join(path, 'setup.py')
    args = [os.path.abspath(filename), 'dist_info']
    with open(filename, 'r') as f:
        has_shebang = f.readline().startswith('#!')
    is_executable = (os.stat(filename).st_mode & 0o100 != 0)
    if not has_shebang or not is_executable:
        # TODO(jelmer): Why python3 and not e.g. python?
        args.insert(0, 'python3')

    with tempfile.TemporaryDirectory() as td:
        try:
            subprocess.call(
                args, cwd=td, stderr=subprocess.PIPE,
                stdout=subprocess.PIPE)
        except FileNotFoundError:
            pass
        return read_python_pkg_info(td)


def guess_from_debian_watch(path, trust_package):
    with open(path, 'r') as f:
        wf = parse_watch_file(f)
        if not wf:
            return
        for w in wf:
            if w.url.startswith('https://') or w.url.startswith('http://'):
                repo = guess_repo_from_url(w.url)
                if repo:
                    yield "Repository", sanitize_vcs_url(repo), "possible"
                    break
            m = re.match('https?://sf.net/([^/]+)', w.url)
            if m:
                yield "Archive", "SourceForge", "certain"
                yield "X-SourceForge-Project", m.group(1), "certain"
                # TODO(jelmer): Yield sourceforge project name later
                # so something later can call get_sf_metadata ?


def guess_from_debian_control(path, trust_package):
    with open(path, 'r') as f:
        from debian.deb822 import Deb822
        control = Deb822(f)
    if 'Homepage' in control:
        repo = guess_repo_from_url(control['Homepage'])
        if repo:
            yield 'Repository', sanitize_vcs_url(repo), "possible"
    if 'XS-Go-Import-Path' in control:
        yield (
            'Repository',
            sanitize_vcs_url('https://' + control['XS-Go-Import-Path']),
            'possible')


def guess_from_setup_py(path, trust_package):
    try:
        pkg_info = get_python_pkg_info(
            os.path.dirname(path), trust_package=trust_package)
    except FileNotFoundError:
        pass
    else:
        if pkg_info.name:
            yield 'Name', pkg_info.name, 'certain'
        if pkg_info.home_page:
            repo = guess_repo_from_url(pkg_info.home_page)
            if repo:
                yield 'Repository', sanitize_vcs_url(repo), 'possible'
        for value in pkg_info.project_urls:
            url_type, url = value.split(', ')
            if url_type in ('GitHub', 'Repository', 'Source Code'):
                yield 'Repository', sanitize_vcs_url(url), 'certain'


def guess_from_package_json(path, trust_package):
    import json
    with open(path, 'r') as f:
        package = json.load(f)
    if 'name' in package:
        yield 'Name', package['name'], 'certain'
    if 'homepage' in package:
        yield 'Homepage', package['homepage'], 'certain'
    if 'repository' in package:
        if isinstance(package['repository'], dict):
            repo_url = package['repository'].get('url')
        elif isinstance(package['repository'], str):
            repo_url = package['repository']
        else:
            repo_url = None
        if repo_url:
            parsed_url = urlparse(repo_url)
            if parsed_url.scheme and parsed_url.netloc:
                yield 'Repository', sanitize_vcs_url(repo_url), 'certain'
            else:
                # Some people seem to default github. :(
                repo_url = 'https://github.com/' + parsed_url.path
                yield 'Repository', sanitize_vcs_url(repo_url), 'possible'
    if 'bugs' in package:
        yield 'Bugs-Database', package['bugs'], 'certain'


def guess_from_package_xml(path, trust_package):
    import xml.etree.ElementTree as ET
    tree = ET.parse(path)
    root = tree.getroot()
    assert root.tag in (
        'package', '{http://pear.php.net/dtd/package-2.0}package',
        '{http://pear.php.net/dtd/package-2.1}package',
        ), 'root tag is %r' % root.tag
    name_tag = root.find('name')
    if name_tag is not None:
        yield 'Name', name_tag.text, 'certain'
    for url_tag in root.findall('url'):
        if url_tag.get('type') == 'repository':
            yield 'Repository', sanitize_vcs_url(url_tag.text), 'certain'
        if url_tag.get('type') == 'bugtracker':
            yield 'Bug-Database', url_tag.text, 'certain'


def guess_from_dist_ini(path, trust_package):
    from configparser import (
        RawConfigParser,
        NoSectionError,
        ParsingError,
        NoOptionError,
        )
    parser = RawConfigParser(strict=False)
    with open(path, 'r') as f:
        try:
            parser.read_string('[START]\n' + f.read())
        except ParsingError as e:
            warn('Unable to parse dist.ini: %r' % e)
    try:
        yield 'Name', parser['START']['name'], 'certain'
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield ('Bug-Database',
               parser['MetaResources']['bugtracker.web'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield ('Repository',
               parser['MetaResources']['repository.url'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass


def guess_from_debian_copyright(path, trust_package):
    from debian.copyright import (
        Copyright,
        NotMachineReadableError,
        MachineReadableFormatError,
        )
    with open(path, 'r') as f:
        try:
            copyright = Copyright(f, strict=False)
        except NotMachineReadableError:
            header = None
        except MachineReadableFormatError as e:
            warn('Error parsing copyright file: %s' % e)
            header = None
        else:
            header = copyright.header
    if header:
        if header.upstream_name:
            yield "Name", header.upstream_name, 'certain'
        if header.upstream_contact:
            yield "Contact", ','.join(header.upstream_contact), 'certain'
        if header.source:
            if ' 'in header.source:
                from_url = re.split('[ ,]', header.source)[0]
            else:
                from_url = header.source
            repo_url = guess_repo_from_url(from_url)
            if repo_url:
                yield 'Repository', sanitize_vcs_url(repo_url), 'possible'
        if "X-Upstream-Bugs" in header:
            yield "Bug-Database", header["X-Upstream-Bugs"], 'certain'
        if "X-Source-Downloaded-From" in header:
            yield "Repository", guess_repo_from_url(
                header["X-Source-Downloaded-From"]), 'certain'


def guess_from_readme(path, trust_package):
    import shlex
    try:
        with open(path, 'rb') as f:
            for line in f:
                line = line.decode('utf-8', 'replace')
                if line.strip().startswith('git clone'):
                    line = line.strip()
                    argv = shlex.split(line)
                    args = [arg for arg in argv[2:] if not arg.startswith('-')]
                    url = args[0]
                    yield ('Repository', sanitize_vcs_url(url), 'possible')
    except IsADirectoryError:
        pass


def guess_from_meta_json(path, trust_package):
    import json
    with open(path, 'r') as f:
        data = json.load(f)
        if 'name' in data:
            yield 'Name', data['name'], 'certain'
        if 'resources' in data:
            resources = data['resources']
            if 'bugtracker' in resources and 'web' in resources['bugtracker']:
                yield "Bug-Database", resources["bugtracker"]["web"], 'certain'
                # TODO(jelmer): Support resources["bugtracker"]["mailto"]
            if 'homepage' in resources:
                yield "Homepage", resources["homepage"], 'certain'
            if 'repository' in resources:
                repo = resources['repository']
                if 'url' in repo:
                    yield (
                        'Repository', sanitize_vcs_url(repo["url"]), 'certain')
                if 'web' in repo:
                    yield 'Repository-Browse', repo['web'], 'certain'


def guess_from_meta_yml(path, trust_package):
    """Guess upstream metadata from a META.yml file.

    See http://module-build.sourceforge.net/META-spec-v1.4.html for the
    specification of the format.
    """
    import ruamel.yaml
    with open(path, 'r') as f:
        data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
        if 'name' in data:
            yield 'Name', data['name'], 'certain'
        if 'resources' in data:
            resources = data['resources']
            if 'bugtracker' in resources:
                yield 'Bug-Database', resources['bugtracker'], 'certain'
            if 'homepage' in resources:
                yield 'Homepage', resources['homepage'], 'certain'
            if 'repository' in resources:
                yield (
                    'Repository', sanitize_vcs_url(resources['repository']),
                    'certain')


def guess_from_doap(path, trust_package):
    """Guess upstream metadata from a DOAP file.
    """
    from xml.etree import ElementTree
    el = ElementTree.parse(path)
    root = el.getroot()
    DOAP_NAMESPACE = 'http://usefulinc.com/ns/doap#'
    if root.tag != ('{%s}Project' % DOAP_NAMESPACE):
        warn('Doap file does not have DOAP project as root')
        return

    def extract_url(el):
        return el.attrib.get(
            '{http://www.w3.org/1999/02/22-rdf-syntax-ns#}resource')

    for child in root:
        if child.tag == ('{%s}name' % DOAP_NAMESPACE):
            yield 'Name', child.text, 'certain'
        if child.tag == ('{%s}bug-database' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield 'Bug-Database', url, 'certain'
        if child.tag == ('{%s}homepage' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield 'Homepage', url, 'certain'
        if child.tag == ('{%s}repository' % DOAP_NAMESPACE):
            for repo in child:
                if repo.tag in (
                        '{%s}SVNRepository' % DOAP_NAMESPACE,
                        '{%s}GitRepository' % DOAP_NAMESPACE):
                    repo_location = repo.find(
                        '{http://usefulinc.com/ns/doap#}location')
                    if repo_location is not None:
                        repo_url = extract_url(repo_location)
                    else:
                        repo_url = None
                    if repo_url:
                        yield (
                            'Repository', sanitize_vcs_url(repo_url),
                            'certain')
                    web_location = repo.find(
                        '{http://usefulinc.com/ns/doap#}browse')
                    if web_location is not None:
                        web_url = extract_url(web_location)
                    else:
                        web_url = None

                    if web_url:
                        yield 'Repository-Browse', web_url, 'certain'


def guess_from_configure(path, trust_package=False):
    with open(path, 'rb') as f:
        for line in f:
            if b'=' not in line:
                continue
            (key, value) = line.strip().split(b'=', 1)
            if b' ' in key:
                continue
            if b'$' in value:
                continue
            value = value.strip()
            if value.startswith(b"'") and value.endswith(b"'"):
                value = value[1:-1]
            if not value:
                continue
            if key == b'PACKAGE_NAME':
                yield 'Name', value.decode(), 'certain'
            elif key == b'PACKAGE_BUGREPORT':
                yield 'Bug-Submit', value.decode(), 'certain'
            elif key == b'PACKAGE_URL':
                yield 'Homepage', value.decode(), 'certain'


def guess_from_r_description(path, trust_package=False):
    with open(path, 'r') as f:
        from debian.deb822 import Deb822
        description = Deb822(f)
        if 'Package' in description:
            yield 'Name', description['Package'], 'certain'
        if 'Repository' in description:
            yield 'Archive', description['Repository'], 'certain'
        if 'BugReports' in description:
            yield 'Bug-Database', description['BugReports'], 'certain'
        if 'URL' in description:
            entries = [entry.strip()
                       for entry in re.split('[\n,]', description['URL'])]
            urls = []
            for entry in entries:
                m = re.match('([^ ]+) \\((.*)\\)', entry)
                if m:
                    url = m.group(1)
                    label = m.group(2)
                else:
                    url = entry
                    label = None
                urls.append((label, url))
            if len(urls) == 1:
                yield 'Homepage', urls[0][1], 'possible'
            for label, url in urls:
                if label and label.lower() in ('devel', 'repository'):
                    yield 'Repository', url, 'certain'
                elif label and label.lower() in ('homepage', ):
                    yield 'Homepage', url, 'certain'
                else:
                    repo_url = guess_repo_from_url(url)
                    if repo_url:
                        yield 'Repository', repo_url, 'certain'


def guess_upstream_metadata_items(path, trust_package=False,
                                  minimum_certainty=None):
    """Guess upstream metadata items, in no particular order.

    Args:
      path: Path to the package
      trust: Whether to trust the package contents and i.e. run
      executables in it
    Yields:
      Tuples with (key, value, certainty)
    """
    CANDIDATES = [
        ('debian/watch', guess_from_debian_watch),
        ('debian/control', guess_from_debian_control),
        ('setup.py', guess_from_setup_py),
        ('package.json', guess_from_package_json),
        ('package.xml', guess_from_package_xml),
        ('dist.ini', guess_from_dist_ini),
        ('debian/copyright', guess_from_debian_copyright),
        ('META.json', guess_from_meta_json),
        ('META.yml', guess_from_meta_yml),
        ('configure', guess_from_configure),
        ('DESCRIPTION', guess_from_r_description),
        ]

    doap_filenames = [n for n in os.listdir(path) if n.endswith('.doap')]
    if doap_filenames:
        if len(doap_filenames) == 1:
            CANDIDATES.append((doap_filenames[0], guess_from_doap))
        else:
            warn('More than one doap filename, ignoring all: %r' %
                 doap_filenames)

    CANDIDATES.extend([
        ('README', guess_from_readme),
        ('README.md', guess_from_readme),
        ])

    for relpath, guesser in CANDIDATES:
        abspath = os.path.join(path, relpath)
        if not os.path.exists(abspath):
            continue
        for key, value, certainty in guesser(
                abspath, trust_package=trust_package):
            if certainty == 'possible' and minimum_certainty == 'certain':
                continue
            yield key, value, certainty

    # TODO(jelmer): validate Repository by querying it somehow?


def guess_upstream_metadata(path, trust_package=False):
    """Guess the upstream metadata dictionary.

    Args:
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
    """
    current_certainty = {}
    code = {}
    for key, value, certainty in guess_upstream_metadata_items(
            path, trust_package=trust_package):
        if current_certainty.get(key) != 'certain':
            code[key] = value
            current_certainty[key] = certainty

    extend_upstream_metadata(code, current_certainty)
    return code


def extend_upstream_metadata(code, certainty):
    """Extend a set of upstream metadata.
    """
    if 'Repository' in code and 'Repository-Browse' not in code:
        browse_url = browse_url_from_repo_url(code['Repository'])
        if browse_url:
            code['Repository-Browse'] = browse_url
            certainty['Repository-Browse'] = certainty['Repository']
    # TODO(jelmer): Try deriving bug-database too?


if __name__ == '__main__':
    import argparse
    import sys
    import ruamel.yaml
    parser = argparse.ArgumentParser(sys.argv[0])
    parser.add_argument('path')
    args = parser.parse_args()

    metadata = guess_upstream_metadata(args.path)
    sys.stdout.write(ruamel.yaml.round_trip_dump(metadata))
