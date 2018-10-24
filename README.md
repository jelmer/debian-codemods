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

Supported tags
--------------

The current set of lintian tags for which a fixer is available that can fix a
subset of the issues:

* ancient-python-version-field
* control-file-with-CRLF-EOLs
* copyright-has-crs
* debian-rules-should-not-use-pwd
* debian-upstream-obsolete-path
* debian-watch-uses-insecure-uri
* dm-upload-allowed-is-obsolete
* file-contains-trailing-whitespace
* homepage-field-uses-insecure-uri
* insecure-copyright-format-uri
* malformed-dm-upload-allowed
* missing-debian-source-format
* out-of-date-copyright-format-uri
* package-uses-deprecated-source-override-location
* quilt-series-without-trailing-newline
* unnecessary-testsuite-autopkgtest-field
* vcs-field-uses-insecure-uri
* vcs-field-uses-not-recommended-uri-format
* wrong-debian-qa-group-name
* xc-package-type-in-debian-control
* xs-testsuite-field-in-debian-control
* xs-vcs-field-in-debian-control

Writing new fixers
------------------

Each fixer is a simple script that lives under ``fixers/lintian``. The script
should be named after the tag it fixes. File extensions are ignored.

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
