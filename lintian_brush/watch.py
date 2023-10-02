#!/usr/bin/python3
# Copyright (C) 2018-2020 Jelmer Vernooij <jelmer@debian.org>
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

"""Functions for working with watch files."""

import json
import logging
import os
import re
import urllib.error
from collections import namedtuple
from contextlib import suppress
from dataclasses import dataclass
from typing import List, Optional, Set
from urllib.parse import urlparse, urlunparse
from urllib.request import Request, urlopen

from debmutate.reformatting import FormattingUnpreservable
from debmutate.versions import get_snapshot_revision, strip_dfsg_suffix
from debmutate.watch import (
    Watch,
    WatchEditor,
    WatchFile,
    apply_url_mangle,
)

from debian.changelog import Changelog

from . import (
    DEFAULT_URLLIB_TIMEOUT,
    USER_AGENT,
    certainty_to_confidence,
    min_certainty,
)
from .gpg import fetch_keys
from .svp import svp_enabled

# TODO(jelmer): Vary this depending on whether a new watch file was added or an
# existing one was fixed?
WATCH_FIX_VALUE = 60


@dataclass
class WatchCandidate:

    watch: Watch
    site: str
    certainty: Optional[str]
    preference: Optional[int]


class KeyRetrievalFailed(Exception):

    def __init__(self, fingerprints):
        self.fingerprints = fingerprints


COMMON_PGPSIGURL_MANGLES = [
    's/$/.%s/' % ext for ext in ['asc', 'pgp', 'gpg', 'sig', 'sign']]


SignatureInfo = namedtuple('SignatureInfo', ['is_valid', 'keys', 'mangle'])


def probe_signature(r, *, pgpsigurlmangle=None, mangles=None,
                    gpg_context=None):
    """Try to find the signature file for a release."""
    import gpg.errors

    if gpg_context is None:
        import gpg
        gpg_context = gpg.Context(armor=True)

    if mangles is None:
        mangles = COMMON_PGPSIGURL_MANGLES

    def sig_valid(sig):
        return sig.status == 0

    if r.pgpsigurl:
        pgpsigurls = [(pgpsigurlmangle, r.pgpsigurl)]
    else:
        pgpsigurls = [
            (mangle, apply_url_mangle(mangle, r.url))
            for mangle in mangles]
    for mangle, pgpsigurl in pgpsigurls:
        # Try and download signatures from some predictable locations.
        try:
            resp = urlopen(pgpsigurl)
        except urllib.error.HTTPError:
            continue
        except TimeoutError:
            logging.warning('Timeout error retrieving %s', pgpsigurl)
            continue
        sig = resp.read()
        actual = urlopen(r.url).read()
        try:
            gr = gpg_context.verify(actual, sig)[1]
        except gpg.errors.GPGMEError as e:
            logging.warning(
                'Error verifying signature %s on %s: %s',
                pgpsigurl, r.url, e)
            continue
        except gpg.errors.BadSignatures as e:
            if str(e).endswith(': No public key'):
                if not fetch_keys(
                        [s.fpr for s in e.result.signatures],
                        home_dir=gpg_context.home_dir):
                    logging.warning(
                        'Unable to retrieve keys: %r',
                        e.result.signatures)
                    raise KeyRetrievalFailed(
                        [s.fpr for s in e.result.signatures]) from e
                gr = gpg_context.verify(actual, sig)[1]
            else:
                raise
        signatures = gr.signatures
        is_valid = True
        needed_keys = set()
        for sig in signatures:
            if not sig_valid(sig):
                logging.warning(
                    'Signature from %s in %s for %s not valid',
                    sig.fpr, pgpsigurl, r.url)
                is_valid = False
            else:
                needed_keys.add(sig.fpr)
        return SignatureInfo(
            is_valid=is_valid, keys=needed_keys, mangle=mangle)
    else:
        return None


def candidates_from_setup_py(
        path, good_upstream_versions: Set[str], net_access=False):
    certainty = "likely"
    # Import setuptools in case it replaces distutils
    with suppress(ImportError):
        import setuptools  # noqa: F401
    from distutils.core import run_setup

    try:
        result = run_setup(os.path.abspath(path), stop_after="config")
    except BaseException:  # noqa: PIE786
        import traceback

        traceback.print_exc()
        return
    project = result.get_name()  # type: ignore
    version = result.get_version()  # type: ignore
    if not project:
        logging.warning('no project name in setup.py')
        return
    current_version_filenames = None
    if net_access:
        # TODO(jelmer): Use ognibuild.upstream.load_pypi
        json_url = "https://pypi.python.org/pypi/%s/json" % project
        logging.info('Getting %s info on pypi (%s)',
                     project, json_url)
        pypi_data = _load_json(json_url)
        if pypi_data is None:
            logging.warning('unable to find project %s on pypi',
                            project)
            return
        if version in pypi_data["releases"]:
            release = pypi_data["releases"][version]
            current_version_filenames = [
                (d["filename"], d["has_sig"])
                for d in release
                if d["packagetype"] == "sdist"
            ]
    filename_regex = (
        fr"{project}-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))")
    opts = []
    # TODO(jelmer): Set uversionmangle?
    # opts.append('uversionmangle=s/(rc|a|b|c)/~$1/')
    if current_version_filenames:
        for (fn, has_sig) in current_version_filenames:
            if re.match(filename_regex, fn):
                certainty = "certain"
                if has_sig:
                    opts.append("pgpsigurlmangle=s/$/.asc/")
    url = fr"https://pypi.debian.net/{project}/{filename_regex}"
    # TODO(jelmer): Add pgpsigurlmangle if has_sig==True
    w = Watch(url, opts=opts)
    yield WatchCandidate(w, "pypi", certainty=certainty, preference=1)


def find_candidates(path, good_upstream_versions, net_access=False):
    candidates = []
    if os.path.exists(os.path.join(path, 'setup.py')):
        candidates.extend(candidates_from_setup_py(
            os.path.join(path, 'setup.py'), good_upstream_versions,
            net_access=net_access))

    if os.path.exists(os.path.join(path, 'debian/upstream/metadata')):
        candidates.extend(candidates_from_upstream_metadata(
            os.path.join(path, 'debian/upstream/metadata'),
            good_upstream_versions, net_access=net_access))

    def candidate_key(candidate):
        return (
            certainty_to_confidence(candidate.certainty),
            candidate.preference)

    candidates.sort(key=candidate_key)

    return candidates


def candidates_from_upstream_metadata(
        path: str, good_upstream_versions: Set[str], net_access: bool = False):
    try:
        with open(path) as f:
            inp = f.read()
    except FileNotFoundError:
        pass
    else:
        import ruamel.yaml

        code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)

        for field in ["Repository", "X-Download"]:
            try:
                parsed_url = urlparse(code[field].split(' ')[0])
            except KeyError:
                pass
            else:
                if parsed_url.hostname == "github.com":
                    yield from guess_github_watch_entry(
                        parsed_url, good_upstream_versions,
                        net_access=net_access
                    )
                if parsed_url.hostname == "launchpad.net":
                    yield from guess_launchpad_watch_entry(
                        parsed_url, good_upstream_versions,
                        net_access=net_access
                    )

        archive = code.get('Archive')
        if archive == 'CRAN':
            yield from guess_cran_watch_entry(code['Name'])


def guess_cran_watch_entry(name):
    w = Watch(r'https://cran.r-project.org/src/contrib/%s_([-\d.]*)\.tar\.gz'
              % name)
    yield WatchCandidate(w, "cran", certainty="likely", preference=0)


def guess_launchpad_watch_entry(
        parsed_url, good_upstream_versions, net_access=False):
    if not net_access:
        return
    project = parsed_url.path.strip("/").split("/")[0]
    url = f"https://api.launchpad.net/devel/{project}/releases"
    entries = []
    while url:
        response = _load_json(url)
        entries.extend(response['entries'])
        url = response.get('next_collection_link')
    files = _load_json(entries[-1]['files_collection_link'])
    assert len(files['entries']) > 0
    # TODO(jelmer): add
    filepattern = files['entries'][0]['file_link'].split('/')[-2].replace(
        entries[-1]['version'], '(.*)')
    w = Watch(
        f'https://launchpad.net/{project}/+download',
        f'https://launchpad.net/{project}/.*/{filepattern}')
    yield WatchCandidate(w, "launchpad", certainty="certain", preference=0)


def guess_github_watch_entry(
        parsed_url, good_upstream_versions, net_access=False):
    import re

    from breezy.branch import Branch

    if not net_access:
        return
    branch = Branch.open(urlunparse(parsed_url))
    tags = branch.tags.get_tag_dict()
    POSSIBLE_PATTERNS = [r"v(\d\S+)", r"(\d\S+)",
                         r".*/[vV]?(\d[^\s+]+)\.tar\.gz"]
    version_pattern = None
    # TODO(jelmer): Maybe use releases API instead?
    # TODO(jelmer): Automatically added mangling for
    # e.g. rc and beta
    uversionmangle: List[str] = []
    for name in sorted(tags, reverse=True):
        for pattern in POSSIBLE_PATTERNS:
            m = re.match(pattern, name)
            if not m:
                continue
            if m.group(1) in good_upstream_versions:
                version_pattern = pattern
                break
        if version_pattern:
            break
    else:
        return
    (username, project) = parsed_url.path.strip("/").split("/")
    if project.endswith(".git"):
        project = project[:-4]
    download_url = f"https://github.com/{username}/{project}/tags"
    matching_pattern = r".*\/%s\.tar\.gz" % version_pattern
    opts = [
        fr"filenamemangle=s/{matching_pattern}/{project}-$1\.tar\.gz/"
    ]
    if uversionmangle:
        opts.append(r"uversionmangle=" + ";".join(uversionmangle))
    # TODO(jelmer): Check for GPG
    # opts.append(
    #    r'pgpsigurlmangle='
    #    r's/archive\/%s\.tar\.gz/releases\/download\/$1\/$1\.tar\.gz\.asc/' %
    #    version_pattern)
    w = Watch(download_url, matching_pattern, opts=opts)
    yield WatchCandidate(w, "github", certainty="certain", preference=0)


def _load_json(url):
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    try:
        response = urlopen(
            Request(url, headers=headers), timeout=DEFAULT_URLLIB_TIMEOUT
        )
    except urllib.error.HTTPError as e:
        if e.status == 404:
            return None
        raise
    return json.load(response)


def candidates_from_hackage(package, good_upstream_versions, net_access=False):
    if not net_access:
        return
    url = "https://hackage.haskell.org/package/%s/preferred" % package
    versions = _load_json(url)
    if versions is None:
        return
    for version in versions["normal-version"]:
        if version in good_upstream_versions:
            break
    else:
        return
    download_url = "https://hackage.haskell.org/package/" + package
    matching_pattern = r".*/%s-(.*).tar.gz" % package
    w = Watch(download_url, matching_pattern)
    yield WatchCandidate(w, "hackage", certainty="certain", preference=1)


def fix_old_github_patterns(updater):
    ret = []
    for w in getattr(updater.watch_file, "entries", []):
        parsed_url = urlparse(w.url)

        # only applies to github.com
        if parsed_url.netloc != "github.com":
            continue

        parts = parsed_url.path.strip('/').split('/')
        if len(parts) >= 3 and parts[2] in ('tags', 'releases'):  # noqa:SIM114
            pass
        elif len(parts) >= 2 and parts[0] == '.*' and parts[1] == 'archive':
            pass
        else:
            continue

        parts = w.matching_pattern.split('/')
        if len(parts) > 2 and parts[-2] == 'archive':
            parts.insert(-1, 'refs/tags')
        w.matching_pattern = '/'.join(parts)
        ret.append(w)
    return ret


def fix_github_releases(updater):
    ret = []
    for w in getattr(updater.watch_file, "entries", []):
        parsed_url = urlparse(w.url)

        # only applies to github.com
        if parsed_url.netloc != "github.com":
            continue

        parts = parsed_url.path.strip('/').split('/')
        if len(parts) >= 3 and parts[2] == 'releases':
            parts[2] = 'tags'
            parsed_url = parsed_url._replace(path='/'.join(parts))
            w.url = parsed_url.geturl()
            ret.append(w)
    return ret


def fix_watch_issues(updater):
    ret = []
    ret.extend(fix_old_github_patterns(updater))
    ret.extend(fix_github_releases(updater))
    return ret


def watch_entries_certainty(entries, source_package,
                            expected_versions=None,
                            default_certainty="likely"):
    certainty = "certain"
    for entry in entries:
        try:
            ret = verify_watch_entry(
                entry, source_package,
                expected_versions=expected_versions)
        except WatchEntryVerificationFailure:
            certainty = min_certainty(["possible", certainty])
        except TemporaryWatchEntryVerficationError:
            certainty = min_certainty([default_certainty, certainty])
        else:
            if not ret:
                certainty = min_certainty(["possible", certainty])
    return certainty


class WatchEntryVerificationFailure(Exception):
    """Failure verifying watch entry."""


class TemporaryWatchEntryVerficationError(Exception):
    """Temporary error verifying watch entry."""


class WatchEntryVerificationStatus:

    def __init__(self, entry, releases, missing_versions=None):
        self.entry = entry
        self.releases = {r.version: r for r in releases}
        self.missing_versions = missing_versions

    def __bool__(self):
        return not self.missing_versions and bool(self.releases)


def verify_watch_entry(
        entry: Watch, source_package: str,
        expected_versions: Optional[
            List[str]] = None) -> WatchEntryVerificationStatus:
    try:
        releases = list(sorted(
                entry.discover(source_package), reverse=True))
    except urllib.error.HTTPError as e:
        logging.warning(
            'HTTP error accessing discovery URL %s: %s.',
            e.geturl(), e)
        if (e.status or 0) // 100 == 5:
            # If the server is unhappy, then the entry could still be valid.
            raise TemporaryWatchEntryVerficationError(str(e)) from e

        raise WatchEntryVerificationFailure(str(e)) from e

    if expected_versions is None:
        return WatchEntryVerificationStatus(entry, releases=releases)

    found_versions = {str(r.version) for r in releases}
    missing_versions = set(expected_versions) - found_versions
    return WatchEntryVerificationStatus(
        entry, releases=releases, missing_versions=missing_versions)


def report_fatal(code: str, description: str, context=None, hint=None) -> None:
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'description': description,
                'context': context}, f)
    logging.fatal('%s', description)
    if hint:
        logging.info('%s', hint)


def verify_watch_file(watch_file, source_package, expected_versions):
    ret = []
    for entry in getattr(watch_file, 'entries', []):
        ret.append(verify_watch_entry(
            entry, source_package, expected_versions))
    return ret


def svp_context(status, site):
    return [{
        'entries': {
            'text': str(entry_status.entry),
            'site': entry_site,
            'releases': {
                str(r.version): {
                    'url': r.url,
                } for r in entry_status.releases.values()
            },
            'missing_versions':
                [str(x) for x in entry_status.missing_versions]
        }
    } for entry_status, entry_site in zip(
        status or [], site or [None] * len(status or []))]


_changelog_policy_noted = False


def _note_changelog_policy(policy, msg):
    global _changelog_policy_noted
    if not _changelog_policy_noted:
        if policy:
            extra = "Specify --no-update-changelog to override."
        else:
            extra = "Specify --update-changelog to override."
        logging.info("%s %s", msg, extra)
    _changelog_policy_noted = True


def main():  # noqa: C901
    import argparse
    import logging

    import breezy  # noqa: E402

    breezy.initialize()  # type: ignore
    import breezy.bzr  # noqa: E402
    import breezy.git  # noqa: E402
    from breezy.commit import NullCommitReporter, PointlessCommit
    from breezy.workingtree import WorkingTree
    from breezy.workspace import (
        WorkspaceDirty,
        check_clean_tree,
    )

    from . import (
        get_committer,
        version_string,
    )
    from .config import Config

    parser = argparse.ArgumentParser(prog="deb-update-watch")
    parser.add_argument(
        "--directory",
        metavar="DIRECTORY",
        help="directory to run in",
        type=str,
        default=".",
    )
    parser.add_argument(
        "--no-update-changelog",
        action="store_false",
        default=None,
        dest="update_changelog",
        help="do not update the changelog",
    )
    parser.add_argument(
        "--update-changelog",
        action="store_true",
        dest="update_changelog",
        help="force updating of the changelog",
        default=None,
    )
    parser.add_argument(
        "--allow-reformatting",
        default=None,
        action="store_true",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument(
        "--identity",
        help="Print user identity that would be used when committing",
        action="store_true",
        default=False,
    )
    parser.add_argument(
        "--debug", help="Describe all considered changes.", action="store_true"
    )
    parser.add_argument(
        "--force",
        help="Force regenerating watch file, even if it the current one works",
        action="store_true")
    parser.add_argument(
        "--no-verify", action="store_true",
        help="Do not verify that the new watch file works")
    parser.add_argument(
        "--disable-net-access",
        help="Do not probe external services.",
        action="store_true",
        default=False,
    )

    args = parser.parse_args()

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format='%(message)s')

    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        logging.info('%s', get_committer(wt))
        return 0

    with wt.lock_write():
        try:
            check_clean_tree(wt, wt.basis_tree(), subpath)
        except WorkspaceDirty:
            logging.info("%s: Please commit pending changes first.",
                         wt.basedir)
            return 1

        update_changelog = args.update_changelog
        allow_reformatting = args.allow_reformatting
        try:
            cfg = Config.from_workingtree(wt, subpath)
        except FileNotFoundError:
            pass
        else:
            if update_changelog is None:
                update_changelog = cfg.update_changelog()
            if allow_reformatting is None:
                allow_reformatting = cfg.allow_reformatting()

        if allow_reformatting is None:
            allow_reformatting = False

        good_upstream_versions = set()

        changelog_path = os.path.join(subpath, "debian/changelog")

        with wt.get_file(changelog_path) as f:
            cl = Changelog(f, max_blocks=1)
            for entry in cl:
                uv = strip_dfsg_suffix(entry.version.upstream_version)
                if get_snapshot_revision(uv) is not None:
                    continue
                good_upstream_versions.add(uv)
            package = cl.package

        expected_versions = list(sorted(good_upstream_versions))[-5:]

        status = None
        try:
            with WatchEditor(allow_reformatting=allow_reformatting,
                             allow_missing=False,
                             ) as updater:
                if not args.force:
                    try:
                        status = verify_watch_file(
                            updater.watch_file, package, expected_versions)
                    except (TemporaryWatchEntryVerficationError,
                            WatchEntryVerificationFailure):
                        status = [None]
                    if status and all(status):
                        hint = (
                            'Releases {} can be found with watch entry {}'
                            .format(
                                ', '.join(sorted(list(
                                    status[0].releases.keys()), reverse=True)),
                                status[0].entry))
                        report_fatal(
                            'nothing-to-do',
                            'Existing watch file has valid entries',
                            context=svp_context(status, site=None),
                            hint=hint)
                        return 0
                    fix_watch_issues(updater)
                    try:
                        status = verify_watch_file(
                            updater.watch_file, package, expected_versions)
                    except WatchEntryVerificationFailure:
                        status = [None]
                    except TemporaryWatchEntryVerficationError as e:
                        report_fatal('temporary-verification-error', str(e))
                        return 1
                if status is None or not all(status):
                    candidates = find_candidates(
                        '.', good_upstream_versions,
                        net_access=not args.disable_net_access)
                    if len(candidates) == 0:
                        report_fatal(
                            'no-download-urls-found',
                            'No candidates for the watch file were found')
                        return 1
                    updater.allow_reformatting = True
                    updater.watch_file.entries = [candidates[0].watch]
                    site = [candidates[0].site]
                    status = None
                    summary = f'Update watch file from {site[0]}'
                else:
                    site = None
                    summary = 'Fixed watch file'
            specific_files = updater.changed_files
        except FileNotFoundError:
            candidates = find_candidates(
                '.', good_upstream_versions,
                net_access=not args.disable_net_access)
            wf = WatchFile()
            if len(candidates) == 0:
                report_fatal(
                    'no-download-urls-found',
                    'No candidates for the watch file were found')
                return 1
            wf.entries.append(candidates[0].watch)
            site = [candidates[0].site]

            with open('debian/watch', 'w') as f:
                wf.dump(f)
            status = None
            summary = f'Added watch file from {site}'
            specific_files = ['debian/watch']
        except FormattingUnpreservable as e:
            report_fatal('formatting-unpreservable',
                         "Unable to preserve formatting of %s" % e.path)
            if hasattr(e, 'diff'):  # debmutate >= 0.64
                sys.stderr.writelines(e.diff())
            return 1

        if not args.no_verify and status is None:
            with WatchEditor() as updater:
                try:
                    status = verify_watch_file(
                        updater.watch_file, package, expected_versions)
                except WatchEntryVerificationFailure as e:
                    report_fatal(
                        'verification-failed',
                        'Unable to verify watch entry: %s'
                        % e)
                    return 1
                except TemporaryWatchEntryVerficationError as e:
                    report_fatal(
                        'temporary-verification-error',
                        'Unable to verify watch entry: %s'
                        % e)
                    return 1
                if not status or not all(status):
                    report_fatal(
                        'verification-failed',
                        'Unable to watch entries; missing versions: %r'
                        % status[0].missing_versions,
                        context=svp_context(status, site))
                    return 1

        if update_changelog is None:
            from .detect_gbp_dch import guess_update_changelog

            dch_guess = guess_update_changelog(
                wt, os.path.join(subpath, 'debian'))
            if dch_guess:
                update_changelog = dch_guess.update_changelog
                _note_changelog_policy(update_changelog, dch_guess.explanation)
            else:
                # Assume we should update changelog
                update_changelog = True

        if update_changelog:
            from .changelog import add_changelog_entry

            # TODO(jelmer): Add note here about having verified watch file?
            add_changelog_entry(wt, changelog_path, [summary])
            specific_files.append(changelog_path)

        committer = get_committer(wt)

        with suppress(PointlessCommit):
            wt.commit(
                specific_files=specific_files,
                message=f"{summary}\n\nChanges-By: fix-watch-file",
                allow_pointless=False,
                reporter=NullCommitReporter(),
                committer=committer,
            )

    if svp_enabled():
        with open(os.environ["SVP_RESULT"], "w") as f:
            json.dump({
                "description": summary,
                "context": svp_context(status, site),
                "value": WATCH_FIX_VALUE,
            }, f)

    logging.info('%s', summary)

    if status is not None:
        logging.info('Releases {} can be found with new watch entry {}'.format(
            ', '.join(sorted(list(
                status[0].releases.keys()), reverse=True)),
            status[0].entry))

    return 0


if __name__ == '__main__':
    import sys

    sys.exit(main())
