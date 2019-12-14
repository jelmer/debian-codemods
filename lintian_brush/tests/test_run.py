#!/usr/bin/python
# Copyright (C) 2018 Jelmer Vernooij
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

"""Tests for lintian_brush."""

import os
import re
import sys

from debian.changelog import (
    Changelog,
    Version,
    )

from breezy.config import GlobalStack
from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush import (
    Fixer,
    FixerFailed,
    FixerResult,
    FixerScriptFailed,
    NoChanges,
    NotDebianPackage,
    PendingChanges,
    PythonScriptFixer,
    ScriptFixer,
    UnsupportedCertainty,
    available_lintian_fixers,
    check_clean_tree,
    add_changelog_entry,
    guess_update_changelog,
    certainty_sufficient,
    min_certainty,
    certainty_to_confidence,
    confidence_to_certainty,
    get_committer,
    increment_version,
    only_changes_last_changelog_block,
    parse_script_fixer_output,
    run_lintian_fixer,
    run_lintian_fixers,
    select_fixers,
    version_string,
    )


CHANGELOG_FILE = ('debian/changelog', """\
blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")


class AvailableLintianFixersTest(TestCaseWithTransport):

    def test_find_shell_scripts(self):
        self.build_tree([
            'fixers/',
            'fixers/anotherdir/',
            'fixers/i-fix-a-tag.sh',
            'fixers/i-fix-another-tag.py',
            'fixers/.hidden',
            'fixers/backup-file.sh~',
            'fixers/no-extension'])
        self.build_tree_contents([
            ('fixers/index.desc', """\
Fix-Script: foo.sh
Lintian-Tags: i-fix-a-tag

Fix-Script: bar.sh
Lintian-Tags: i-fix-another-tag, no-extension
""")])
        self.assertEqual(
                [['i-fix-a-tag'], ['i-fix-another-tag', 'no-extension']],
                [fixer.lintian_tags
                 for fixer in available_lintian_fixers('fixers')])


class CheckCleanTreeTests(TestCaseWithTransport):

    def make_test_tree(self, format=None):
        tree = self.make_branch_and_tree('.', format=format)
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            CHANGELOG_FILE])
        tree.add(['debian', 'debian/changelog', 'debian/control'])
        tree.commit('Initial thingy.')
        return tree

    def test_pending_changes(self):
        tree = self.make_test_tree()
        self.build_tree_contents([('debian/changelog', 'blah')])
        with tree.lock_write():
            self.assertRaises(
                PendingChanges, check_clean_tree, tree)

    def test_pending_changes_bzr_empty_dir(self):
        # See https://bugs.debian.org/914038
        tree = self.make_test_tree(format='bzr')
        self.build_tree_contents([('debian/upstream/', )])
        with tree.lock_write():
            self.assertRaises(
                PendingChanges, check_clean_tree, tree)

    def test_pending_changes_git_empty_dir(self):
        # See https://bugs.debian.org/914038
        tree = self.make_test_tree(format='git')
        self.build_tree_contents([('debian/upstream/', )])
        with tree.lock_write():
            check_clean_tree(tree)

    def test_pending_changes_git_dir_with_ignored(self):
        # See https://bugs.debian.org/914038
        tree = self.make_test_tree(format='git')
        self.build_tree_contents([
            ('debian/upstream/', ),
            ('debian/upstream/blah', ''),
            ('.gitignore', 'blah\n'),
            ])
        tree.add('.gitignore')
        tree.commit('add gitignore')
        with tree.lock_write():
            check_clean_tree(tree)

    def test_extra(self):
        tree = self.make_test_tree()
        self.build_tree_contents([('debian/foo', 'blah')])
        with tree.lock_write():
            self.assertRaises(
                PendingChanges, check_clean_tree,
                tree)


class DummyFixer(Fixer):

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty, trust_package, allow_reformatting, net_access,
            opinionated):
        with open(os.path.join(basedir, 'debian/control'), 'a') as f:
            f.write('a new line\n')
        return FixerResult("Fixed some tag.\nExtended description.",
                           ['some-tag'], "certain")


class FailingFixer(Fixer):

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty, trust_package, allow_reformatting, net_access,
            opinionated):
        with open(os.path.join(basedir, 'debian/foo'), 'w') as f:
            f.write("blah")
        with open(os.path.join(basedir, 'debian/control'), 'a') as f:
            f.write("foo\n")
        raise FixerFailed("Not successful")


class RunLintianFixerTests(TestCaseWithTransport):

    def make_test_tree(self, format=None):
        tree = self.make_branch_and_tree('.', format=format)
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            CHANGELOG_FILE])
        tree.add(['debian', 'debian/changelog', 'debian/control'])
        tree.commit('Initial thingy.')
        return tree

    def test_not_debian_tree(self):
        tree = self.make_test_tree()
        tree.remove('debian/changelog')
        os.remove('debian/changelog')
        tree.commit("not a debian dir")
        with tree.lock_write():
            self.assertRaises(
                NotDebianPackage, run_lintian_fixer,
                tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)

    def test_simple_modify(self):
        tree = self.make_test_tree()
        with tree.lock_write():
            result, summary = run_lintian_fixer(
                tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Fixed some tag.")
        self.assertEqual(['some-tag'], result.fixed_lintian_tags)
        self.assertEqual('certain', result.certainty)
        self.assertEqual(2, tree.branch.revno())
        self.assertEqual(
                tree.get_file_lines('debian/control')[-1],
                b"a new line\n")

    def test_simple_modify_too_uncertain(self):
        tree = self.make_test_tree()

        class UncertainFixer(Fixer):
            def run(self, basedir, package, current_version, compat_release,
                    minimum_certainty, trust_package, allow_reformatting,
                    net_access, opinionated):
                with open(os.path.join(basedir, 'debian/somefile'), 'w') as f:
                    f.write("test")
                return FixerResult("Renamed a file.", certainty='possible')
        with tree.lock_write():
            self.assertRaises(
                NoChanges, run_lintian_fixer,
                tree, UncertainFixer('dummy', 'some-tag'),
                update_changelog=False, minimum_certainty='certain')
        self.assertEqual(1, tree.branch.revno())

    def test_simple_modify_acceptably_uncertain(self):
        tree = self.make_test_tree()

        class UncertainFixer(Fixer):
            def run(self, basedir, package, current_version, compat_release,
                    minimum_certainty, trust_package, allow_reformatting,
                    net_access, opinionated):
                with open(os.path.join(basedir, 'debian/somefile'), 'w') as f:
                    f.write("test")
                return FixerResult("Renamed a file.", certainty='possible')
        with tree.lock_write():
            result, summary = run_lintian_fixer(
                tree, UncertainFixer('dummy', 'some-tag'),
                update_changelog=False, minimum_certainty='possible')
        self.assertEqual(2, tree.branch.revno())

    def test_new_file(self):
        tree = self.make_test_tree()

        class NewFileFixer(Fixer):
            def run(self, basedir, package, current_version, compat_release,
                    minimum_certainty, trust_package, allow_reformatting,
                    net_access, opinionated):
                with open(os.path.join(basedir, 'debian/somefile'), 'w') as f:
                    f.write("test")
                return FixerResult("Created new file.", ['some-tag'])
        with tree.lock_write():
            result, summary = run_lintian_fixer(
                tree, NewFileFixer('new-file', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Created new file.")
        self.assertIs(None, result.certainty)
        self.assertEqual(['some-tag'], result.fixed_lintian_tags)
        rev = tree.branch.repository.get_revision(
            tree.last_revision())
        self.assertEqual(rev.message, (
            'Created new file.\n'
            '\n'
            'Fixes lintian: some-tag\n'
            'See https://lintian.debian.org/tags/some-tag.html for '
            'more details.\n'))
        self.assertEqual(2, tree.branch.revno())
        basis_tree = tree.branch.basis_tree()
        with basis_tree.lock_read():
            self.assertEqual(
                    basis_tree.get_file_text('debian/somefile'),
                    b"test")

    def test_rename_file(self):
        tree = self.make_test_tree()

        class RenameFileFixer(Fixer):
            def run(self, basedir, package, current_version, compat_release,
                    minimum_certainty, trust_package, allow_reformatting,
                    net_access, opinionated):
                os.rename(os.path.join(basedir, 'debian/control'),
                          os.path.join(basedir, 'debian/control.blah'))
                return FixerResult("Renamed a file.")
        orig_basis_tree = tree.branch.basis_tree()
        with tree.lock_write():
            result, summary = run_lintian_fixer(
                tree, RenameFileFixer('rename', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Renamed a file.")
        self.assertIs(None, result.certainty)
        self.assertEqual([], result.fixed_lintian_tags)
        self.assertEqual(2, tree.branch.revno())
        basis_tree = tree.branch.basis_tree()
        with basis_tree.lock_read(), orig_basis_tree.lock_read():
            self.assertFalse(basis_tree.has_filename('debian/control'))
            self.assertTrue(basis_tree.has_filename('debian/control.blah'))
            self.assertNotEqual(orig_basis_tree.get_revision_id(),
                                basis_tree.get_revision_id())
            self.expectFailure(
                'mv --auto appears to be broken',
                self.assertEqual, basis_tree.path2id('debian/control.blah'),
                orig_basis_tree.path2id('debian/control'))

    def test_empty_change(self):
        tree = self.make_test_tree()

        class EmptyFixer(Fixer):
            def run(self, basedir, package, current_version, compat_release,
                    minimum_certainty, trust_package, allow_reformatting,
                    net_access, opinionated):
                return FixerResult("I didn't actually change anything.")
        with tree.lock_write():
            self.assertRaises(
                    NoChanges, run_lintian_fixer, tree,
                    EmptyFixer('empty', 'some-tag'), update_changelog=False)
        self.assertEqual(1, tree.branch.revno())
        with tree.lock_read():
            self.assertEqual(
                [], list(tree.iter_changes(tree.basis_tree())))

    def test_fails(self):
        tree = self.make_test_tree()
        with tree.lock_write():
            self.assertRaises(
                    Exception, run_lintian_fixer, tree,
                    FailingFixer('fail', 'some-tag'), update_changelog=False)
        self.assertEqual(1, tree.branch.revno())
        with tree.lock_read():
            self.assertEqual(
                [], list(tree.iter_changes(tree.basis_tree())))


class RunLintianFixersTests(TestCaseWithTransport):

    def setUp(self):
        super(RunLintianFixersTests, self).setUp()
        self.tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            CHANGELOG_FILE])
        self.tree.add(['debian', 'debian/changelog', 'debian/control'])
        self.tree.commit('Initial thingy.')

    def test_fails(self):
        with self.tree.lock_write():
            applied, failed = run_lintian_fixers(
                    self.tree, [FailingFixer('fail', 'some-tag')],
                    update_changelog=False)
        self.assertEqual([], applied)
        self.assertEqual({'fail': FixerFailed('Not successful')}, failed)
        with self.tree.lock_read():
            self.assertEqual(
                [], list(self.tree.iter_changes(self.tree.basis_tree())))

    def test_not_debian_tree(self):
        self.tree.remove('debian/changelog')
        os.remove('debian/changelog')
        self.tree.commit("not a debian dir")
        with self.tree.lock_write():
            self.assertRaises(
                NotDebianPackage, run_lintian_fixers,
                self.tree, [DummyFixer('dummy', 'some-tag')],
                update_changelog=False)

    def test_simple_modify(self):
        with self.tree.lock_write():
            applied, failed = run_lintian_fixers(
                self.tree, [DummyFixer('dummy', 'some-tag')],
                update_changelog=False)
        self.assertEqual(
                [(FixerResult("Fixed some tag.\nExtended description.",
                              ['some-tag'], 'certain'),
                  'Fixed some tag.')],
                applied)
        self.assertEqual({}, failed)
        self.assertEqual(2, self.tree.branch.revno())
        self.assertEqual(
                self.tree.get_file_lines('debian/control')[-1],
                b"a new line\n")


class HonorsVcsCommitter(TestCaseWithTransport):

    def make_package_tree(self, format):
        tree = self.make_branch_and_tree('.', format=format)
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            CHANGELOG_FILE])
        tree.add(['debian', 'debian/changelog', 'debian/control'])
        tree.commit('Initial thingy.')
        return tree

    def make_change(self, tree, committer=None):
        with tree.lock_write():
            result, summary = run_lintian_fixer(
                tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False, committer=committer)
        self.assertEqual(summary, "Fixed some tag.")
        self.assertEqual(['some-tag'], result.fixed_lintian_tags)
        self.assertEqual('certain', result.certainty)
        self.assertEqual(2, tree.branch.revno())
        self.assertEqual(
                tree.get_file_lines('debian/control')[-1],
                b"a new line\n")

    def test_honors_tree_committer_specified(self):
        tree = self.make_package_tree('git')
        self.make_change(tree, committer='Jane Example <jane@example.com>')
        rev = tree.branch.repository.get_revision(tree.branch.last_revision())
        self.assertEqual(rev.committer, 'Jane Example <jane@example.com>')

    def test_honors_tree_committer_config(self):
        tree = self.make_package_tree('git')
        with open(os.path.join(tree.basedir, '.git/config'), 'w') as f:
            f.write("""\
[user]
  email = jane@example.com
  name = Jane Example

""")

        self.make_change(tree)
        rev = tree.branch.repository.get_revision(tree.branch.last_revision())
        self.assertEqual(rev.committer, 'Jane Example <jane@example.com>')


class IncrementVersionTests(TestCase):

    def assertVersion(self, expected, start):
        v = Version(start)
        increment_version(v)
        self.assertEqual(Version(expected), v)

    def test_full(self):
        self.assertVersion('1.0-2', '1.0-1')

    def test_native(self):
        self.assertVersion('1.1', '1.0')


class OnlyChangesLastChangelogBlockTests(TestCaseWithTransport):

    def make_package_tree(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            ('debian/changelog', """\
blah (0.2) UNRELEASED; urgency=medium

  * And a change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")])
        tree.add(['debian', 'debian/changelog', 'debian/control'])
        tree.commit('Initial thingy.')
        return tree

    def test_no_changes(self):
        tree = self.make_package_tree()
        self.assertFalse(only_changes_last_changelog_block(
            tree, 'debian/changelog'))

    def test_other_change(self):
        tree = self.make_package_tree()
        self.build_tree_contents([
            ('debian/control', """\
Source: blah
Vcs-Bzr: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

""")])

        self.assertFalse(only_changes_last_changelog_block(
            tree, 'debian/changelog'))

    def test_other_changes(self):
        tree = self.make_package_tree()
        self.build_tree_contents([
            ('debian/control', """\
Source: blah
Vcs-Bzr: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            ('debian/changelog', """\
blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)
  * Some other change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")])
        self.assertFalse(only_changes_last_changelog_block(
            tree, 'debian/changelog'))

    def test_changes_to_other_changelog_entries(self):
        tree = self.make_package_tree()
        self.build_tree_contents([
            ('debian/changelog', """\
blah (0.2) UNRELEASED; urgency=medium

  * debian/changelog: And a change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * debian/changelog: Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")])
        self.assertFalse(only_changes_last_changelog_block(
            tree, 'debian/changelog'))

    def test_changes_to_last_only(self):
        tree = self.make_package_tree()
        self.build_tree_contents([
            ('debian/changelog', """\
blah (0.2) UNRELEASED; urgency=medium

  * And a change.
  * Not a team upload.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")])
        self.assertTrue(only_changes_last_changelog_block(
            tree, 'debian/changelog'))

    def test_changes_to_last_only_but_released(self):
        tree = self.make_package_tree()
        self.build_tree_contents([
            ('debian/changelog', """\
blah (0.2) unstable; urgency=medium

  * And a change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")])
        tree.commit('release')
        self.build_tree_contents([
            ('debian/changelog', """\
blah (0.2) unstable; urgency=medium

  * And a change.
  * Team Upload.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")])
        self.assertFalse(only_changes_last_changelog_block(
            tree, 'debian/changelog'))


class LintianBrushVersion(TestCase):

    def test_matches_package_version(self):
        if not os.path.exists('debian/changelog'):
            self.skipTest(
                'no debian/changelog available. '
                'Running outside of source tree?')
        with open('debian/changelog', 'r') as f:
            cl = Changelog(f, max_blocks=1)
        self.assertEqual(str(cl.version).split('+')[0], version_string)

    def test_matches_setup_version(self):
        if not os.path.exists('setup.py'):
            self.skipTest(
                'no setup.py available. '
                'Running outside of source tree?')
        # TODO(jelmer): Surely there's a better way of doing this?
        with open('setup.py', 'r') as f:
            for l in f:
                m = re.match(r'[ ]*version="(.*)",', l)
                if m:
                    setup_version = m.group(1)
                    break
            else:
                raise AssertionError('setup version not found')
        self.assertEqual(version_string, setup_version)


class GetCommitterTests(TestCaseWithTransport):

    def test_git_env(self):
        self.overrideEnv('GIT_COMMITTER_NAME', 'Some Git Committer')
        self.overrideEnv('GIT_COMMITTER_EMAIL', 'committer@example.com')
        tree = self.make_branch_and_tree('.', format='git')
        self.assertEqual(
            'Some Git Committer <committer@example.com>',
            get_committer(tree))

    def test_git_config(self):
        tree = self.make_branch_and_tree('.', format='git')
        with open('.git/config', 'w') as f:
            f.write("""\
[user]
   name = Some Other Git Committer
   email = other@example.com
""")
        self.assertEqual(
            'Some Other Git Committer <other@example.com>',
            get_committer(tree))

    def test_global_stack(self):
        gs = GlobalStack()
        gs.set('email', 'Yet Another Committer <yet@example.com>')
        tree = self.make_branch_and_tree('.', format='git')
        self.assertEqual(
            'Yet Another Committer <yet@example.com>',
            get_committer(tree))


class CertaintySufficientTests(TestCase):

    def test_sufficient(self):
        self.assertTrue(certainty_sufficient('certain', 'certain'))
        self.assertTrue(certainty_sufficient('certain', 'possible'))
        self.assertTrue(certainty_sufficient('certain', None))
        self.assertTrue(certainty_sufficient('possible', None))
        # TODO(jelmer): Should we really always allow unknown certainties
        # through?
        self.assertTrue(certainty_sufficient(None, 'certain'))

    def test_insufficient(self):
        self.assertFalse(certainty_sufficient('possible', 'certain'))


class CertaintyVsConfidenceTests(TestCase):

    def test_confidence_to_certainty(self):
        self.assertEqual("certain", confidence_to_certainty(0))
        self.assertEqual("confident", confidence_to_certainty(1))
        self.assertEqual("likely", confidence_to_certainty(2))
        self.assertEqual("possible", confidence_to_certainty(3))
        self.assertEqual("unknown", confidence_to_certainty(None))
        self.assertRaises(ValueError, confidence_to_certainty, 2000)

    def test_certainty_to_confidence(self):
        self.assertEqual(0, certainty_to_confidence("certain"))
        self.assertEqual(1, certainty_to_confidence("confident"))
        self.assertEqual(2, certainty_to_confidence("likely"))
        self.assertEqual(3, certainty_to_confidence("possible"))
        self.assertIs(None, certainty_to_confidence("unknown"))
        self.assertRaises(ValueError, certainty_to_confidence, "blah")


class MinimumCertaintyTests(TestCase):

    def test_minimum(self):
        self.assertEqual("certain", min_certainty([]))
        self.assertEqual("certain", min_certainty(["certain"]))
        self.assertEqual("possible", min_certainty(["possible"]))
        self.assertEqual("possible", min_certainty(["possible", "certain"]))
        self.assertEqual("likely", min_certainty(["likely", "certain"]))
        self.assertEqual(
            "possible",
            min_certainty(["likely", "certain", "possible"]))


class ParseScriptFixerOutputTests(TestCase):

    def test_simple(self):
        self.assertEqual(
            FixerResult('Do bla', ['tag-a'], 'certain'),
            parse_script_fixer_output("""\
Do bla
Fixed-Lintian-Tags: tag-a
Certainty: certain
"""))

    def test_unknown_certainty(self):
        self.assertRaises(
            UnsupportedCertainty,
            parse_script_fixer_output, """\
Do bla
Fixed-Lintian-Tags: tag-a
Certainty: blah
""")

    def test_default_certainty(self):
        self.assertEqual(
            FixerResult('Do bla', ['tag-a'], None),
            parse_script_fixer_output("""\
Do bla
Fixed-Lintian-Tags: tag-a
"""))


class BaseScriptFixerTests(object):

    script_fixer_cls = None

    def create_fixer(self, code):
        self.build_tree_contents([
            ('script.py',
             "#!" + sys.executable + "\n" + code)])
        os.chmod('script.py', 0o755)
        fixer = self.script_fixer_cls(
            'fixer', ['a-tag'], os.path.abspath('script.py'))
        self.assertEqual(os.path.abspath('script.py'), fixer.script_path)
        return fixer

    def test_noop(self):
        fixer = self.create_fixer("print('I did not do anything')\n")
        result = fixer.run(self.test_dir, 'blah', '0.1', 'buster')
        self.assertIsInstance(result, FixerResult)
        self.assertEqual(result.description, 'I did not do anything')

    def test_chdir(self):
        fixer = self.create_fixer("import os; print(os.getcwd())\n")
        os.mkdir('subdir')
        os.chdir('subdir')
        result = fixer.run(self.test_dir, 'blah', '0.1', 'buster')
        self.assertIsInstance(result, FixerResult)
        self.assertEqual(result.description, self.test_dir)

    def test_exception(self):
        fixer = self.create_fixer("""\
def foo():
    raise Exception('oops')
foo()
""")
        e = self.assertRaises(
            FixerScriptFailed, fixer.run, self.test_dir, 'blah', '0.1',
            'buster')
        self.assertEqual(e.path, fixer.script_path)
        self.assertEqual(e.returncode, 1)
        self.assertEqual(
            e.errors.splitlines()[-2:],
            ["    raise Exception('oops')", 'Exception: oops'])


class ScriptFixerTests(BaseScriptFixerTests, TestCaseWithTransport):

    script_fixer_cls = ScriptFixer


class PythonScriptFixerTests(BaseScriptFixerTests, TestCaseWithTransport):

    script_fixer_cls = PythonScriptFixer


class SelectFixersTests(TestCase):

    def test_exists(self):
        self.assertEqual(
            ['dummy1'],
            [f.name for f in select_fixers(
                [DummyFixer('dummy1', 'some-tag'),
                 DummyFixer('dummy2', 'other-tag')],
                ['dummy1'])])

    def test_missing(self):
        self.assertRaises(
            KeyError, select_fixers, [DummyFixer('dummy', 'some-tag')],
            ['other'])

    def test_exclude_missing(self):
        self.assertRaises(
            KeyError, select_fixers, [DummyFixer('dummy', 'some-tag')],
            ['dummy'], ['some-other'])

    def test_exclude(self):
        self.assertEqual(
            ['dummy1'],
            [f.name for f in select_fixers(
                [DummyFixer('dummy1', 'some-tag'),
                 DummyFixer('dummy2', 'other-tag')],
                ['dummy1', 'dummy2'], ['dummy2'])])


class ChangelogAddEntryTests(TestCaseWithTransport):

    def test_edit_existing(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
 """)])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        add_changelog_entry(tree, 'debian/changelog', 'Add a foo')
        self.assertFileEqual("""\
lintian-brush (0.35) UNRELEASED; urgency=medium

  [ Joe Example ]
  * Support updating templated debian/control files that use cdbs
    template.

  [ Jane Example ]
  * Add a foo

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_add_new(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) unstable; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
 """)])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        add_changelog_entry(tree, 'debian/changelog', 'Add a foo')
        text = tree.get_file_text('debian/changelog').decode()
        lines = text.splitlines()
        self.assertEqual([
            'lintian-brush (0.36) UNRELEASED; urgency=medium',
            '',
            '  * Add a foo',
            ''],
            lines[:4])
        self.assertTrue(
            lines[4].startswith(' -- Jane Example <jane@example.com>  '),
            lines[4])

    def test_edit_broken_first_line(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
THIS IS NOT A PARSEABLE LINE
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
 """)])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        add_changelog_entry(tree, 'debian/changelog', 'Add a foo')
        self.assertFileEqual("""\
THIS IS NOT A PARSEABLE LINE
lintian-brush (0.35) UNRELEASED; urgency=medium

  [ Joe Example ]
  * Support updating templated debian/control files that use cdbs
    template.

  [ Jane Example ]
  * Add a foo

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')


class GuessUpdateChangelogTests(TestCaseWithTransport):

    def test_no_gbp_conf(self):
        tree = self.make_branch_and_tree('.')
        self.assertTrue(guess_update_changelog(tree))

    def test_gbp_conf_dch(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/gbp.conf', """\
[dch]
""")])
        tree.add(['debian', 'debian/gbp.conf'])
        self.assertFalse(guess_update_changelog(tree))

    def test_changelog_sha_prefixed(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
blah (0.20.1) unstable; urgency=medium

  [ Somebody ]
  * [ebb7c31] do a thing
  * [629746a] do another thing that actually requires us to wrap lines
    and then

  [ Somebody Else ]
  * [b02b435] do another thing

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
""")])
        tree.add(['debian', 'debian/changelog'])
        self.assertFalse(guess_update_changelog(tree))
