lintian-brush
=============

This package contains a set of scripts to automatically fix some common issues in
Debian packages, as reported by Lintian.

Running lintian-brush
---------------------

Simply run::

```shell
lintian-brush
```

in the top-level of your (version controlled) Debian package.

.. _supported-tags:

Supported tags
--------------

The current set of lintian tags for which a fixer is available that can fix a
subset of the issues:

* ancient-python-version-field
* control-file-with-CRLF-EOLs
* copyright-has-crs
* debhelper-but-no-misc-depends
* debian-control-has-empty-field
* debian-control-has-obsolete-dbg-package
* debian-pyversions-is-obsolete
* debian-rules-should-not-use-pwd
* debian-upstream-obsolete-path
* debian-watch-uses-insecure-uri
* dh-quilt-addon-but-quilt-source-format
* dm-upload-allowed-is-obsolete
* file-contains-trailing-whitespace
* homepage-field-uses-insecure-uri
* insecure-copyright-format-uri
* malformed-dm-upload-allowed
* missing-debian-source-format
* missing-vcs-browser-field
* out-of-date-copyright-format-uri
* package-needs-versioned-debhelper-build-depends
* package-uses-deprecated-debhelper-compat-version
* package-uses-deprecated-source-override-location
* priority-extra-is-replaced-by-priority-optional
* public-upstream-key-not-minimal
* quilt-series-without-trailing-newline
* systemd-service-file-pidfile-refers-to-var-run
* transitional-package-should-be-oldlibs-optional
* unnecessary-team-upload
* unnecessary-testsuite-autopkgtest-field
* unversioned-copyright-format-uri
* useless-autoreconf-build-depends
* vcs-field-uses-insecure-uri
* vcs-field-uses-not-recommended-uri-format
* wrong-debian-qa-group-name
* xc-package-type-in-debian-control
* xs-testsuite-field-in-debian-control
* xs-vcs-field-in-debian-control

.. _writing-fixers:

Writing new fixers
------------------

Each fixer is a simple script that lives under ``fixers``. Scripts should
be registered in the ``index.desc`` file in the same directory.

A fixer is run in the root directory of a package, where it can make changes
it deems necessary. If a fixer can not provide any improvements, it can simply
leave the working tree untouched - lintian-brush will not create any commits for it
or update the changelog.

A fixer should write a short description of the changes it has made to standard
out; this will be used for the commit message.

It can include optional metadata in its output::

 * ``Fixes-Lintian-Tags:`` followed by a comma-separated list of lintian tags
   that it claims to have fixed. This will make lintian-brush include
   links to documentation about the fixed lintian tags. In the future,
   it may also support building the package to verify the lintian tag
   is actually resolved.

 * ``Certainty:`` followed by ``certain`` or ``possible``,
   indicating how certain the fixer is that the fix was the right
   one.

The easiest way to test fixers is to create a skeleton *in* and *out* source tree under
``tests/FIXER-NAME/TEST-NAME``. The ``in`` directory should contain the tree to
run the fixer on,and ``out`` contains the directory after it has run. It's fine
to create directories with only one or two control files, if the fixer only
needs those.
