#!/usr/bin/python

from setuptools import setup

setup(
        name="lintian-brush",
        author="Jelmer Vernooij",
        author_email="jelmer@debian.org",
        packages=["lintian_brush"],
        url="https://salsa.debian.org/jelmer/lintian-brush",
        description="Automatic lintian issue fixer",
        project_urls={
            "Repository": "https://salsa.debian.org/jelmer/lintian-brush",
        },
        requires=['breezy', 'debian'],
)
