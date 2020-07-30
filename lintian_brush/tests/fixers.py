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

"""Tests for lintian_brush fixers."""

from debian.changelog import Changelog
import os
import subprocess
import shutil
import tempfile
import unittest

from lintian_brush import (
    available_lintian_fixers,
    load_renamed_tags,
    parse_script_fixer_output,
    select_fixers,
    increment_version,
    )


class FixerTestCase(unittest.TestCase):
    """Test case that runs a fixer test."""

    def __init__(self, fixer, fixer_name, name, path):
        self._fixer = fixer
        self._fixer_name = fixer_name
        self._test_name = name
        self._path = path
        self._testdir = None
        self._tempdir = None
        self.maxDiff = None
        super(FixerTestCase, self).__init__()

    def setUp(self):
        self._tempdir = tempfile.mkdtemp()
        self._testdir = os.path.join(self._tempdir, 'testdir')

        def ignore(src, names):
            return [name for name in names if name.endswith('~')]
        shutil.copytree(os.path.join(self._path, 'in'), self._testdir,
                        symlinks=True, ignore=ignore)

    def id(self):
        return "%s.%s.%s" % (__name__, self._fixer_name, self._test_name)

    def __str__(self):
        return 'fixer test: %s for %s' % (self._test_name, self._fixer_name)

    def tearDown(self):
        shutil.rmtree(self._tempdir)
        self._testdir = None
        self._tempdir = None

    def runTest(self):
        env = dict(os.environ.items())
        cl_path = os.path.join(self._testdir, 'debian/changelog')
        if os.path.exists(cl_path):
            with open(cl_path, 'rb') as f:
                cl = Changelog(f, max_blocks=1)
            if cl.distributions == 'UNRELEASED':
                current_version = cl.version
            else:
                current_version = cl.version
            increment_version(current_version)
        else:
            current_version = '1.0-1'
        env['CURRENT_VERSION'] = str(current_version)
        env['NET_ACCESS'] = 'disallow'
        env['MINIMUM_CERTAINTY'] = 'possible'
        env_path = os.path.join(self._path, 'env')
        if os.path.exists(env_path):
            with open(env_path, 'r') as f:
                for line in f:
                    key, value = line.rstrip('\n').split('=')
                    env[key] = value
        p = subprocess.Popen(self._fixer.script_path, cwd=self._testdir,
                             stdout=subprocess.PIPE,
                             env=env)
        (result, err) = p.communicate("")
        self.assertEqual(p.returncode, 0)
        out_path = os.path.join(self._path, 'out')
        p = subprocess.Popen(
            ['diff', '--no-dereference', '-x', '*~', '-ur',
             os.path.join(self._path, os.readlink(out_path))
             if os.path.islink(out_path) else out_path, self._testdir],
            stdout=subprocess.PIPE)
        (diff, stderr) = p.communicate('')
        self.assertIn(
                p.returncode, (0, 1),
                "Unexpected exit code %d" % p.returncode)
        if diff.decode() != '':
            raise AssertionError("unexpected output: %s" % diff.decode())
        self.assertMultiLineEqual(diff.decode(), '')

        if (not os.path.islink(out_path) or
                os.readlink(os.path.join(self._path, 'out')) != 'in'):
            with open(os.path.join(self._path, 'message'), 'r') as f:
                # Assert that message on stdout matches
                self.assertEqual(
                    result.decode().strip(),
                    f.read().strip())
            result = parse_script_fixer_output(result.decode())
            self.assertTrue(
                set(result.fixed_lintian_tags).issubset(
                    self._fixer.lintian_tags),
                'fixer %s claims to fix tags (%r) not declared '
                'in index.desc (%r)' % (
                    self._fixer_name, result.fixed_lintian_tags,
                    self._fixer.lintian_tags))


class SaneFixerTests(unittest.TestCase):
    """Check that the test is sensible."""

    def id(self):
        return "%s.%s.sane" % (__name__, self.fixer.name)

    def __str__(self):
        return 'fixer sanity test: %s' % (self.fixer.name)

    def __init__(self, fixer):
        self.fixer = fixer
        super(SaneFixerTests, self).__init__()

    def runTest(self):
        self.assertTrue(
            os.path.exists(self.fixer.script_path),
            'Script %s missing' % self.fixer.script_path)

        renames = load_renamed_tags()
        for tag in self.fixer.lintian_tags:
            self.assertNotIn(
                tag, renames,
                'Tag %s has been renamed to %s' % (tag, renames.get(tag)))


def iter_test_cases(fixer):
    test_dir = os.path.join(os.path.dirname(__file__), '..', '..', 'tests')
    testpath = os.path.join(test_dir, fixer.name)
    if not os.path.isdir(testpath):
        return
    for testname in os.listdir(testpath):
        yield FixerTestCase(
            fixer_name=fixer.name,
            fixer=fixer,
            name=testname,
            path=os.path.join(test_dir, fixer.name, testname))


def test_suite():
    suite = unittest.TestSuite()
    fixers = available_lintian_fixers()
    for fixer in fixers:
        for test_case in iter_test_cases(fixer):
            suite.addTest(test_case)
        suite.addTest(SaneFixerTests(fixer))
    return suite


if __name__ == '__main__':
    import argparse
    import sys
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '--fixer', type=str, default=None,
        action='append', help='Fixer for which to run tests.')
    parser.add_argument(
        '--exclude', type=str, action='append',
        help='Exclude a fixer.')
    args = parser.parse_args()

    fixers = list(available_lintian_fixers())
    if args.fixer:
        try:
            fixers = select_fixers(fixers, args.fixer, args.exclude)
        except KeyError as e:
            print("Selected fixer %s does not exist." % (e.args[0]),
                  file=sys.stderr)
            sys.exit(0)

    suite = unittest.TestSuite()
    for fixer in fixers:
        for test_case in iter_test_cases(fixer):
            suite.addTest(test_case)

    runner = unittest.TextTestRunner()
    runner.run(suite)
