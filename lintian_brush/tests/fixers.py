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
        env_path = os.path.join(self._path, 'env')
        if os.path.exists(env_path):
            with open(env_path, 'r') as f:
                for l in f:
                    key, value = l.rstrip('\n').split('=')
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


def test_suite():
    suite = unittest.TestSuite()
    test_dir = os.path.join(os.path.dirname(__file__), '..', '..', 'tests')
    for fixer in available_lintian_fixers():
        testpath = os.path.join(test_dir, fixer.name)
        if not os.path.isdir(testpath):
            continue
        for testname in os.listdir(testpath):
            suite.addTest(FixerTestCase(
                fixer_name=fixer.name,
                fixer=fixer,
                name=testname,
                path=os.path.join(test_dir, fixer.name, testname)))
    return suite
