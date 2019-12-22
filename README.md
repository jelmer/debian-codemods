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
* build-depends-on-build-essential
* build-depends-on-obsolete-package
* built-using-field-on-arch-all-package
* comma-separated-files-in-dep5-copyright
* control-file-with-CRLF-EOLs
* copyright-has-crs
* debhelper-but-no-misc-depends
* debhelper-tools-from-autotools-dev-are-deprecated
* debian-changelog-file-contains-obsolete-user-emacs-settings
* debian-changelog-has-wrong-day-of-week
* debian-control-has-empty-field
* debian-control-has-obsolete-dbg-package
* debian-control-has-unusual-field-spacing
* debian-pycompat-is-obsolete
* debian-pyversions-is-obsolete
* debian-rules-contains-unnecessary-get-orig-source-target
* debian-rules-missing-recommended-target
* debian-rules-not-executable
* debian-rules-sets-dpkg-architecture-variable
* debian-rules-should-not-use-pwd
* debian-rules-uses-unnecessary-dh-argument
* debian-source-options-has-custom-compression-settings
* debian-tests-control-and-control-autodep8
* debian-tests-control-autodep8-is-obsolete
* debian-upstream-obsolete-path
* debian-watch-file-is-missing
* debian-watch-file-uses-deprecated-githubredir
* debian-watch-uses-insecure-uri
* debug-symbol-migration-possibly-complete
* declares-possibly-conflicting-debhelper-compat-versions
* dep5-file-paragraph-references-header-paragraph
* dh-clean-k-is-deprecated
* dh-quilt-addon-but-quilt-source-format
* dm-upload-allowed-is-obsolete
* empty-debian-tests-control
* excessive-priority-for-library-package
* field-name-typo-in-dep5-copyright
* file-contains-trailing-whitespace
* font-package-not-multi-arch-foreign
* global-files-wildcard-not-first-paragraph-in-dep5-copyright
* homepage-field-uses-insecure-uri
* homepage-in-binary-package
* init.d-script-needs-depends-on-lsb-base
* insecure-copyright-format-uri
* invalid-short-name-in-dep5-copyright
* invalid-standards-version
* libmodule-build-perl-needs-to-be-in-build-depends
* maintainer-also-in-uploaders.
* maintainer-script-without-set-e
* malformed-dm-upload-allowed
* missing-built-using-field-for-golang-package
* missing-debian-source-format
* missing-vcs-browser-field
* no-homepage-field
* obsolete-field-in-dep5-copyright
* obsolete-runtime-tests-restriction
* old-fsf-address-in-copyright-file
* older-source-format
* orphaned-package-should-not-have-uploaders
* out-of-date-copyright-format-uri
* out-of-date-standards-version
* package-contains-linda-override
* package-needs-versioned-debhelper-build-depends
* package-uses-deprecated-debhelper-compat-version
* package-uses-deprecated-source-override-location
* package-uses-old-debhelper-compat-version
* patch-file-present-but-not-mentioned-in-series
* possible-missing-colon-in-closes
* priority-extra-is-replaced-by-priority-optional
* public-upstream-key-not-minimal
* public-upstream-keys-in-multiple-locations
* quilt-series-but-no-build-dep
* quilt-series-without-trailing-newline
* renamed-tag
* skip-systemd-native-flag-missing-pre-depends
* space-in-std-shortname-in-dep5-copyright
* systemd-service-file-pidfile-refers-to-var-run
* tab-in-licence-text
* transitional-package-should-be-oldlibs-optional
* unnecessary-team-upload
* unnecessary-testsuite-autopkgtest-field
* unused-build-dependency-on-cdbs
* unused-license-paragraph-in-dep5-copyright
* unversioned-copyright-format-uri
* upstream-metadata-file-is-missing
* useless-autoreconf-build-depends
* uses-debhelper-compat-file
* vcs-field-bitrotted
* vcs-field-mismatch
* vcs-field-not-canonical
* vcs-field-uses-insecure-uri
* vcs-field-uses-not-recommended-uri-format
* vcs-obsolete-in-debian-infrastructure
* wrong-debian-qa-group-name
* wrong-section-according-to-package-name
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
or update the changelog. If exits with a non-zero return code, whatever changes
it has made will be discarded and the fixer will be reported as having failed.

The following additional environment variables are set:

 * ``PACKAGE``: The name of the source package that is being edited.
 * ``CURRENT_VERSION``: Package version that is being edited.
 * ``COMPAT_RELEASE``: Debian release to be compatible with. Usually ``sid``
   when --modern was specified and the name of the current stable release otherwise.
 * ``NET_ACCESS``: Whether the fixer is allowed to make network connections
   (e.g. sending HTTP requests). Used by --disable-net-access and the testsuite.
   Set to either ``allow`` or ``disallow``.
 * ``OPINIONATED``: Set to ``yes`` or ``no``. If ``no``, fixers are not expected
   to make changes in which there is no obvious single correct fix.

A fixer should write a short description of the changes it has made to standard
out; this will be used for the commit message.

It can include optional metadata in its output::

 * ``Fixes-Lintian-Tags:`` followed by a comma-separated list of lintian tags
   that it claims to have fixed. This will make lintian-brush include
   links to documentation about the fixed lintian tags. In the future,
   it may also support building the package to verify the lintian tag
   is actually resolved.

 * ``Certainty:`` followed by ``certain``, ``confident``, ``likely`` or
   ``possible``, indicating how certain the fixer is that the fix was the right
   one.

The easiest way to test fixers is to create a skeleton *in* and *out* source tree under
``tests/FIXER-NAME/TEST-NAME``. The ``in`` directory should contain the tree to
run the fixer on,and ``out`` contains the directory after it has run. It's fine
to create directories with only one or two control files, if the fixer only
needs those.
