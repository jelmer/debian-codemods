lintian-brush
=============

This package contains a set of scripts to automatically fix some common issues in
Debian packages.

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

* autotools-pkg-config-macro-not-cross-compilation-safe
* copyright-does-not-refer-to-common-license-file
* copyright-file-contains-full-apache-2-license
* copyright-file-contains-full-gfdl-license
* copyright-file-contains-full-gpl-license
* copyright-not-using-common-license-for-apache2
* copyright-not-using-common-license-for-gfdl
* copyright-not-using-common-license-for-gpl
* copyright-not-using-common-license-for-lgpl
* debian-rules-missing-recommended-target
* debian-rules-parses-dpkg-parsechangelog
* debian-rules-sets-dpkg-architecture-variable
* debian-rules-uses-unnecessary-dh-argument
* debian-watch-file-is-missing
* debug-symbol-migration-possibly-complete
* desktop-entry-contains-encoding-key
* dh-quilt-addon-but-quilt-source-format
* extended-description-is-empty
* initial-upload-closes-no-bugs
* license-file-listed-in-debian-copyright
* missing-build-dependency-for-dh-addon
* missing-build-dependency-for-dh_-command
* missing-prerequisite-for-pyproject-backend
* misspelled-closes-bug
* newer-debconf-templates
* no-copyright-file
* obsolete-vim-addon-manager
* package-uses-deprecated-debhelper-compat-version
* package-uses-old-debhelper-compat-version
* pkg-js-tools-test-is-missing
* possible-missing-colon-in-closes
* recommended-field
* required-field
* silent-on-rules-requiring-root
* skip-systemd-native-flag-missing-pre-depends
* space-in-std-shortname-in-dep5-copyright
* systemd-service-alias-without-extension
* systemd-service-file-refers-to-obsolete-bindto
* systemd-service-file-refers-to-obsolete-target
* systemd-service-file-refers-to-var-run
* systemd-service-file-shutdown-problems
* tab-in-license-text
* typo-in-debhelper-override-target
* upstream-metadata-file-is-missing
* upstream-metadata-missing-bug-tracking
* upstream-metadata-missing-repository
* upstream-metadata-not-yaml-mapping
* upstream-metadata-yaml-invalid
* useless-autoreconf-build-depends
* vcs-field-bitrotted
* vcs-field-uses-insecure-uri
* vcs-obsolete-in-debian-infrastructure
* wrong-section-according-to-package-name

.. _writing-fixers:

Writing new fixers
------------------

For a more extensive write-up, see the
[guide on writing fixers](doc/fixer-writing-guide.rst).

Ideally, fixers target a particular set of lintian tags. This is not strictly
required, but makes it possible to easily find all packages that a particular
fixer can be used on.

Each fixer is a simple script that lives under ``fixers``. Scripts should
be registered in the ``index.desc`` file in the same directory.

A fixer is run in the root directory of a package, where it can make changes
it deems necessary. If a fixer can not provide any improvements, it can simply
leave the working tree untouched - lintian-brush will not create any commits for it
or update the changelog. If exits with a non-zero return code, whatever changes
it has made will be discarded and the fixer will be reported as having failed.

The following additional environment variables are set:

 * ``DEB_SOURCE``: The name of the source package that is being edited.
 * ``CURRENT_VERSION``: Package version that is being edited.
 * ``COMPAT_RELEASE``: Debian release to be compatible with. Usually ``sid``
   when --modern was specified and the name of the current stable release otherwise.
 * ``NET_ACCESS``: Whether the fixer is allowed to make network connections
   (e.g. sending HTTP requests). Used by --disable-net-access and the testsuite.
   Set to either ``allow`` or ``disallow``.
 * ``OPINIONATED``: Set to ``yes`` or ``no``. If ``no``, fixers are not expected
   to make changes in which there is no obvious single correct fix.

For fixer written in python, the ``lintian_brush.fixer`` module can be used for
convenient access to these variables.

A fixer should write a short description of the changes it has made to standard
out; this will be used for the commit message.

It can include optional metadata in its output::

 * ``Fixed-Lintian-Tags:`` followed by a comma-separated list of lintian tags
   that it claims to have fixed. This will make lintian-brush include
   links to documentation about the fixed lintian tags. In the future,
   it may also support building the package to verify the lintian tag
   is actually resolved.

 * ``Certainty:`` followed by ``certain``, ``confident``, ``likely`` or
   ``possible``, indicating how certain the fixer is that the fix was the right
   one.

The default minimum certainty level is "certain"; any incorrect change made
with certainty "certain" is considered *at least* a normal-severity bug.

The easiest way to test fixers is to create a skeleton *in* and *out* source
tree under ``tests/FIXER-NAME/TEST-NAME``. The ``in`` directory should contain
the tree to run the fixer on, and ``out`` contains the directory after it has
run. It's fine to create directories with only one or two control files, if the
fixer only needs those. To run the tests for a single fixer, you can use "make
check-fixer-$NAME".

GitHub Action
-------------

If you're hosting a Git repository on GitHub, you can use the [lintian-brush
GitHub action](https://github.com/gizmoguy/action-lintian-brush) to
automatically run lintian-brush.
