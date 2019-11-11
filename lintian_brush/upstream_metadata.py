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
import urllib.error
from urllib.parse import urlparse, urlunparse, urljoin
from warnings import warn

from debian.deb822 import Deb822

from lintian_brush import (
    USER_AGENT,
    DEFAULT_URLLIB_TIMEOUT,
    certainty_sufficient,
    certainty_to_confidence,
    )
from lintian_brush.vcs import (
    browse_url_from_repo_url,
    plausible_url as plausible_vcs_url,
    sanitize_url as sanitize_vcs_url,
    probe_vcs_url,
    )
from lintian_brush.watch import parse_watch_file
from urllib.request import urlopen, Request


KNOWN_HOSTING_SITES = [
    'code.launchpad.net', 'github.com', 'launchpad.net']
KNOWN_GITLAB_SITES = ['gitlab.com', 'salsa.debian.org']


# See https://wiki.debian.org/UpstreamMetadata
# Supported fields:
# - Homepage
# - Name
# - Contact
# - Repository
# - Repository-Browse
# - Bug-Database
# - Bug-Submit
# - Screenshots
# - Archive
# - X-SourceForge-Project
# - X-Wiki
# - X-Summary

# Supported, but unused.
# - FAQ
# - Donation
# - Documentation
# - Registration
# - Security-Contact
# - Webservice


def _load_json_url(http_url):
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    http_contents = urlopen(
        Request(http_url, headers=headers),
        timeout=DEFAULT_URLLIB_TIMEOUT).read()
    return json.loads(http_contents)


class NoSuchSourceForgeProject(Exception):

    def __init__(self, project):
        self.project = project


def get_sf_metadata(project):
    try:
        return _load_json_url(
            'https://sourceforge.net/rest/p/%s' % project)
    except urllib.error.HTTPError as e:
        if e.status != 404:
            raise
        raise NoSuchSourceForgeProject(project)


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
    if parsed_url.netloc == 'git.savannah.gnu.org':
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) != 2 or path_elements[0] != 'git':
            return None
        return url
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


def update_from_guesses(code, current_certainty, guessed_items):
    fields = set()
    for key, value, certainty in guessed_items:
        if key not in current_certainty or (
                certainty_to_confidence(certainty) <
                certainty_to_confidence(current_certainty[key])):
            if code.get(key) != value:
                code[key] = value
                fields.add(key)
            current_certainty[key] = certainty
    return fields


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
        except subprocess.CalledProcessError:
            if args[0] == 'python3':
                args[0] = 'python2'
            else:
                raise
            subprocess.call(
                args, cwd=td, stderr=subprocess.PIPE,
                stdout=subprocess.PIPE)
        return read_python_pkg_info(td)


def guess_from_debian_watch(path, trust_package):
    def get_package_name():
        with open(os.path.join(os.path.dirname(path), 'control'), 'r') as f:
            return Deb822(f)['Source']
    with open(path, 'r') as f:
        wf = parse_watch_file(f)
        if not wf:
            return
        for w in wf:
            url = w.format_url(package=get_package_name)
            if url.startswith('https://') or url.startswith('http://'):
                repo = guess_repo_from_url(url)
                if repo:
                    yield "Repository", sanitize_vcs_url(repo), "likely"
                    break
            m = re.match('https?://sf.net/([^/]+)', url)
            if m:
                yield "Archive", "SourceForge", "certain"
                yield "X-SourceForge-Project", m.group(1), "certain"


def guess_from_debian_control(path, trust_package):
    with open(path, 'r') as f:
        control = Deb822(f)
    if 'Homepage' in control:
        repo = guess_repo_from_url(control['Homepage'])
        if repo:
            yield 'Repository', sanitize_vcs_url(repo), "likely"
    if 'XS-Go-Import-Path' in control:
        yield (
            'Repository',
            sanitize_vcs_url('https://' + control['XS-Go-Import-Path']),
            'likely')


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
                yield 'Repository', sanitize_vcs_url(repo), 'likely'
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
                # Some people seem to default to github. :(
                repo_url = 'https://github.com/' + parsed_url.path
                yield 'Repository', sanitize_vcs_url(repo_url), 'likely'
    if 'bugs' in package:
        if isinstance(package['bugs'], dict):
            url = package['bugs'].get('url')
        else:
            url = package['bugs']
        if url:
            yield 'Bugs-Database', url, 'certain'


def guess_from_package_xml(path, trust_package):
    import xml.etree.ElementTree as ET
    try:
        tree = ET.parse(path)
    except ET.ParseError as e:
        warn('Unable to parse package.xml: %s' % e)
        return
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
                yield 'Repository', sanitize_vcs_url(repo_url), 'likely'
        if "X-Upstream-Bugs" in header:
            yield "Bug-Database", header["X-Upstream-Bugs"], 'certain'
        if "X-Source-Downloaded-From" in header:
            yield "Repository", guess_repo_from_url(
                header["X-Source-Downloaded-From"]), 'certain'


def guess_from_readme(path, trust_package):
    import shlex
    urls = []
    try:
        with open(path, 'rb') as f:
            lines = list(f.readlines())
            for i, line in enumerate(lines):
                if line.strip().startswith(b'git clone'):
                    line = line.strip()
                    while line.endswith(b'\\'):
                        line += lines[i+1]
                        line = line.strip()
                        i += 1
                    argv = shlex.split(line.decode('utf-8', 'replace'))
                    args = [arg for arg in argv[2:]
                            if not arg.startswith('-') and arg.strip()]
                    try:
                        url = args[-2]
                    except IndexError:
                        url = args[0]
                    if plausible_vcs_url(url):
                        urls.append(sanitize_vcs_url(url))
                m = re.match(
                    b'.*\\(https://travis-ci.org/([^/]+)/([^/]+)\\)', line)
                if m:
                    yield 'Repository', 'https://github.com/%s/%s' % (
                        m.group(1).decode(), m.group(2).decode()), 'possible'
    except IsADirectoryError:
        pass

    def prefer_public(url):
        parsed_url = urlparse(url)
        if 'ssh' in parsed_url.scheme:
            return 1
        return 0
    urls.sort(key=prefer_public)
    if urls:
        yield ('Repository', urls[0], 'possible')


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
    import ruamel.yaml.reader
    with open(path, 'r') as f:
        try:
            data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
        except ruamel.yaml.reader.ReaderError as e:
            warn('Unable to parse META.yml: %s' % e)
            return
        if 'name' in data:
            yield 'Name', data['name'], 'certain'
        if 'resources' in data:
            resources = data['resources']
            if 'bugtracker' in resources:
                yield 'Bug-Database', resources['bugtracker'], 'certain'
            if 'homepage' in resources:
                yield 'Homepage', resources['homepage'], 'certain'
            if 'repository' in resources:
                if isinstance(resources['repository'], dict):
                    url = resources['repository'].get('url')
                else:
                    url = resources['repository']
                if url:
                    yield ('Repository', sanitize_vcs_url(url), 'certain')


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
            if not certainty_sufficient(certainty, minimum_certainty):
                continue
            yield key, value, certainty


def guess_upstream_metadata(path, trust_package=False, net_access=False):
    """Guess the upstream metadata dictionary.

    Args:
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
      net_access: Whether to allow net access
    """
    current_certainty = {}
    code = {}
    update_from_guesses(
        code, current_certainty,
        guess_upstream_metadata_items(
            path, trust_package=trust_package))

    extend_upstream_metadata(
        code, current_certainty, path, net_access=net_access)
    return code


def _possible_fields_missing(code, certainty, fields, field_certainty):
    for field in fields:
        if field not in code:
            return True
        if certainty[field] != 'certain':
            return True
    else:
        return False


def guess_from_sf(sf_project):
    data = get_sf_metadata(sf_project)
    if 'name' in data:
        yield 'Name', data['name']
    if 'external_homepage' in data:
        yield 'Homepage', data['external_homepage']
    if 'screenshots' in data:
        screenshot_urls = [s['url'] for s in data['screenshots'] if 'url' in s]
        if screenshot_urls:
            yield ('Screenshots', screenshot_urls)
    VCS_NAMES = ['bzr', 'hg', 'git']
    vcs_tools = [
        tool for tool in data.get('tools', []) if tool['name'] in VCS_NAMES]
    if len(vcs_tools) == 1:
        yield 'Repository', urljoin('https://sf.net/', vcs_tools[0]['url'])


def extend_from_external_guesser(
        code, certainty, guesser_certainty, guesser_fields, guesser):
    fields = set()
    if not _possible_fields_missing(
            code, certainty, guesser_fields, guesser_certainty):
        return fields

    fields.update(update_from_guesses(
        code, certainty,
        [(key, value, guesser_certainty) for (key, value) in guesser]))

    return fields


def extend_from_sf(code, certainty, sf_project):
    # The set of fields that sf can possibly provide:
    sf_fields = ['Homepage', 'Screenshots', 'Name']

    return extend_from_external_guesser(
        code, certainty, certainty['Archive'], sf_fields,
        guess_from_sf(sf_project))


def extend_from_lp(code, certainty, minimum_certainty, package,
                   distribution=None, suite=None):
    # The set of fields that Launchpad can possibly provide:
    lp_fields = ['Homepage', 'Repository', 'Name']
    lp_certainty = 'possible'

    if minimum_certainty and minimum_certainty != 'possible':
        # Don't bother talking to launchpad if we're not
        # speculating.
        return set()

    return extend_from_external_guesser(
        code, certainty, lp_certainty, lp_fields, guess_from_launchpad(
             package, distribution=distribution, suite=suite))


def extend_upstream_metadata(code, certainty, path, minimum_certainty=None,
                             net_access=False):
    """Extend a set of upstream metadata.
    """
    fields = set()
    if (code.get('Archive') == 'SourceForge' and
            'X-SourceForge-Project' in code and
            net_access):
        sf_project = code['X-SourceForge-Project']
        try:
            fields.update(extend_from_sf(code, certainty, sf_project))
        except NoSuchSourceForgeProject:
            del code['X-SourceForge-Project']
            del certainty['X-SourceForge-Project']
            if 'X-SourceForge-Project' in fields:
                fields.remove('X-SourceForge-Project')
    if net_access:
        with open(os.path.join(path, 'debian/control'), 'r') as f:
            package = Deb822(f)['Source']
        fields.update(extend_from_lp(
            code, certainty, minimum_certainty, package))
    if 'Repository' in code and 'Repository-Browse' not in code:
        browse_url = browse_url_from_repo_url(code['Repository'])
        if browse_url:
            code['Repository-Browse'] = browse_url
            certainty['Repository-Browse'] = certainty['Repository']
            fields.add('Repository-Browse')
    # TODO(jelmer): Try deriving bug-database too?
    return fields


def check_upstream_metadata(code, certainty, version=None):
    """Check upstream metadata.

    This will make network connections, etc.
    """
    if 'Repository' in code and certainty['Repository'] == 'likely':
        if probe_vcs_url(code['Repository'], version=version):
            certainty['Repository'] = 'certain'
        else:
            # TODO(jelmer): Remove altogether, or downgrade to a lesser
            # certainty?
            pass


def guess_from_launchpad(package, distribution=None, suite=None):
    if distribution is None:
        # Default to Ubuntu; it's got more fields populated.
        distribution = 'ubuntu'
    if suite is None:
        if distribution == 'ubuntu':
            from distro_info import UbuntuDistroInfo
            suite = UbuntuDistroInfo().devel()
        elif distribution == 'debian':
            suite = 'sid'
    sourcepackage_url = (
        'https://api.launchpad.net/devel/%(distribution)s/'
        '%(suite)s/+source/%(package)s' % {
            'package': package,
            'suite': suite,
            'distribution': distribution})
    try:
        sourcepackage_data = _load_json_url(sourcepackage_url)
    except urllib.error.HTTPError as e:
        if e.status != 404:
            raise
        return

    productseries_url = sourcepackage_data.get('productseries_link')
    if not productseries_url:
        return
    productseries_data = _load_json_url(productseries_url)
    project_link = productseries_data['project_link']
    project_data = _load_json_url(project_link)
    if project_data.get('homepage_url'):
        yield 'Homepage', project_data['homepage_url']
    yield 'Name', project_data['display_name']
    if project_data.get('sourceforge_project'):
        yield ('X-SourceForge-Project', project_data['sourceforge_project'])
    if project_data.get('wiki_url'):
        yield ('X-Wiki', project_data['wiki_url'])
    if project_data.get('summary'):
        yield ('X-Summary', project_data['summary'])
    if project_data['vcs'] == 'Bazaar':
        branch_link = productseries_data.get('branch_link')
        if branch_link:
            try:
                code_import_data = _load_json_url(
                    branch_link + '/+code-import')
                if code_import_data['url']:
                    # Sometimes this URL is not set, e.g. for CVS repositories.
                    yield 'Repository', code_import_data['url']
            except urllib.error.HTTPError as e:
                if e.status != 404:
                    raise
                if project_data['official_codehosting']:
                    branch_data = _load_json_url(branch_link)
                    if branch_data:
                        yield 'Archive', 'launchpad'
                        yield 'Repository', branch_data['bzr_identity']
                        yield 'Repository-Browse', branch_data['web_link']
    elif project_data['vcs'] == 'Git':
        repo_link = (
            'https://api.launchpad.net/devel/+git?ws.op=getByPath&path=%s' %
            project_data['name'])
        repo_data = _load_json_url(repo_link)
        if not repo_data:
            return
        code_import_link = repo_data.get('code_import_link')
        if code_import_link:
            code_import_data = _load_json_url(repo_data['code_import_link'])
            if code_import_data['url']:
                # Sometimes this URL is not set, e.g. for CVS repositories.
                yield 'Repository', code_import_data['url']
        else:
            if project_data['official_codehosting']:
                yield 'Archive', 'launchpad'
                yield 'Repository', repo_data['git_https_url']
                yield 'Repository-Browse', repo_data['web_link']
    elif project_data.get('vcs') is not None:
        raise AssertionError('unknown vcs: %r' % project_data['vcs'])


if __name__ == '__main__':
    import argparse
    import sys
    import ruamel.yaml
    parser = argparse.ArgumentParser(sys.argv[0])
    parser.add_argument('path', default='.', nargs='?')
    parser.add_argument(
        '--trust',
        action='store_true',
        help='Whether to allow running code from the package.')
    parser.add_argument(
        '--disable-net-access',
        help='Do not probe external services.',
        action='store_true', default=False)
    args = parser.parse_args()

    metadata = guess_upstream_metadata(
        args.path, args.trust, not args.disable_net_access)
    sys.stdout.write(ruamel.yaml.round_trip_dump(metadata))
