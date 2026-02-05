Lintian-Brush: Guide to writing a new fixer
#############################################
:date: 2020-10-12 15:39:00
:author: Jelmer Vernooĳ

lintian-brush can currently fix about 150 different issues that lintian can
report, but that's still a small fraction of the more than thousand different
types of issue that lintian can detect.

This guide explains how to add a fixer in lintian-brush for a lintian tag.

You will need a git checkout of lintian-brush, so let's start with that:

.. code-block:: shell-session

    $ debcheckout lintian-brush
    declared git repository at https://salsa.debian.org/jelmer/lintian-brush.git
    git clone https://salsa.debian.org/jelmer/lintian-brush.git lintian-brush ...
    Cloning into 'lintian-brush'...


Explore Tag Characteristics
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Both the `public UDD mirror <https://udd-mirror.debian.net/>`_ and the `lintian
website <https://lintian.debian.org/>`_ are great resources, and can be helpful
in finding out more about individual tags and which packages are affected
by a tag.

The `tag-status.yaml
<https://salsa.debian.org/jelmer/lintian-brush/-/blob/master/tag-status.yaml>`_
file in lintian-brush documents which tags are currently
implemented or (if they're not implemented) what it would take to implement them.

Find Tags To Fix
~~~~~~~~~~~~~~~~

The `debbugs page for lintian-brush <https://bugs.debian.org/lintian-brush>`_ is a
good place to find ideas for lintian tags to add fixers for.

Alternatively, lintian-brush bundles a ``next.py`` script that can list all tags that
are currently not fixable by lintian-brush, sorted by number of packages
in the archive affected. tags are also categorized by difficulty
(the difficulties are listed in ``tag-status.yaml``).

At the moment, the output for me looks something like this:

.. code-block:: shell-session

    $ python3 next.py
    debian-watch-does-not-check-gpg-signature unknown 25869/27082
    package-contains-documentation-outside-usr-share-doc unknown 4166/439493
    maintainer-manual-page unknown 3336/7908
    executable-in-usr-lib unknown 3166/27378
    typo-in-manual-page medium 3013/12841
    no-symbols-control-file unknown 2862/6634
    breakout-link unknown 2728/7264
    no-dh-sequencer unknown 2655/2815
    quilt-patch-missing-description unknown 2599/6461
    debian-rules-uses-as-needed-linker-flag unknown 1991/2345
    package-does-not-install-examples unknown 1981/3320
    team/pkg-perl/vcs/no-team-url easy 1931/3866
    malformed-override unknown 1926/3168
    national-encoding unknown 1914/18375
    spelling-error-in-copyright unknown 1680/2375
    ...

(the last pair of numbers are the number of packages affected and total number
of issues reported respectively - some tags appear multiple times per package)

Note that you may have to scroll through this list for a bit to find good candidate
tags.

There are some important things to consider when identifying tags to fix:

Correctness
-----------

Changes should be made with a high degree of certainty about correctness. The
tool allows fixers to report the degree of certainty with which a fix has
been made, but by default only high-certainty changes are included.

Currently supported values for certainty are:

* **certain**: this is the correct fix; an incorrect change with this certainty
  is a bug in lintian-brush
* **confident**: almost certainly correct; could be incorrect for one or two
  unusual packages in the archive
* **likely**: likely correct, but there is some uncertainty - e.g. because this
  needs to be verified using network access but that was disabled, or because
* **possible**: this is probably the correct change, but there are known
  situations in which it won't be; needs to be manually verified by a human

While not ideal, it is okay for a fixer to fail and e.g. raise an exception in
unusual circumstances.

Opportunism
-----------

lintian-brush is opportunistic - it is not meant to fix all instances of an
issue, and it's fine for it to abstain when it can't deal with a particular
situation. Often, it's possible to easily fix 80% of instances and
almost impossible to fix the remaining 20% - that's still a big win.
For example, it can fix some tags when they show up in packages
that use debhelper, but not for packages that use cdbs - and that's fine.

Ambiguity
---------

lintian-brush works best for situations where there is one way of fixing
an issue. If there is ambiguity that needs human judgement to resolve or
where the maintainer could have a strong preference for either option,
then it's probably not a good fit.

Note that there is an opionated mode (disabled by default) that users can
enable that will make choices that are correct but may be against the
maintainer's preferences.

Scrolling through the list of tags to fix, it looks like
*team/pkg-perl/vcs/no-team-url* is a good candidate - and actually already
marked as difficulty easy.

Usually I start by reading more about the tag, and by exploring the lintian
source code that triggers it.

In this case, the `lintian description
<https://lintian.debian.org/tags/team/pkg-perl/vcs/no-team-url.html>`_ is quite
brief but clear:

.. class:: italic

   All pkg-perl VCS repositories should live under a team-writable location.

The code in lintian for detecting the tag is quite straightforward as well; I
usually grep for the tag name under checks/ in the lintian source. This tag is emitted from
`checks/team/pkg-perl/vcs.pm <https://salsa.debian.org/lintian/lintian/-/blob/master/checks/team/pkg-perl/vcs.pm>`_.
Looking through the code, it's fairly obvious what it's doing:

* It checks that the package is perl-team maintained
* It emits no-git if there are any non-Git Vcs fields
* It emits no-team-url for any Vcs-Git or Vcs-Browser URLs that aren't under https://salsa.debian.org/perl-team/modules/packages

Okay, that's something we can work with; both of the things it checks for we
can fix with a high degree of certainty.

Writing Test Cases
~~~~~~~~~~~~~~~~~~

Let's add a few tests in lintian-brush to verify that the fixer does what it
needs. There is a directory with test cases for each fixer that lives under
*tests/FIXER-NAME/TESTCASE-NAME* in the lintian-brush source.

Each test directory has the same contents:

 * an ``in`` directory with sparse package contents, the "before" state
 * an ``out`` directory with sparse package contents, the expected state after
   the fixer has been run. This can be a symlink to ``in`` if
   the fixer is not meant to make any changes for this test case.
 * an optional ``message`` file with the expected out from the fixer,
   (if ``out`` is different from ``in``)
 * an optional ``env`` file with environment variables to set
   (formatted as simple key-value pairs, separated by a "=")

For this specific fixer, we'd want to test at least the following scenarios:

 1) A package that is not pkg-perl maintained should remain untouched
 2) A package that is already correct should be untouched
 3) A package that has a non-Vcs-{Git,Browser} header set should have it removed
 4) A package that does not have a Vcs-Git URL set should have it set
 5) A package that has an incorrect Vcs-Git URL set should have it correct
 6) If an override exists for the no-git tag, it should be honored
 7) If an override exists for the no-team-url tag, it should be honored

The first case is the simplest, so let's start with that:

.. code-block:: shell-session

    $ mkdir -p tests/pkg-perl-vcs/not-perl
    $ cd tests/pkg-perl-vcs/not-perl

Since we're not expecting any changes to be made, we can just symlink ``in`` to ``out``.

.. code-block:: shell-session

    $ mkdir in
    $ ln -s in out

The fixer will need to check it the package is maintained
the perl team, so let's add a skeleton package with at least the maintainer
field:

.. code-block:: shell-session

    $ mkdir in/debian
    $ cat <<EOF>in/debian/control
    Source: blah
    Maintainer: Jelmer Vernooij <jelmer@debian.org>
    Vcs-Git: https://salsa.debian.org/jelmer/blah

    Package: blah
    Description: dummy package
    EOF

Second, let's add a test for a package that's already correct:

.. code-block:: shell-session

    $ mkdir -p tests/pkg-perl-vcs/already-correct
    $ cd tests/pkg-perl-vcs/already-correct
    $ mkdir in
    $ ln -s in out
    $ mkdir in/debian
    $ cat <<EOF>in/debian/control
    Source: libblah-perl
    Maintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>
    Vcs-Git: https://salsa.debian.org/perl-team/modules/packages/libblah-perl
    Vcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libblah-perl.git

    Package: libblah-perl
    Description: dummy package
    EOF

And then, one for actually fixing a missing URL:

.. code-block:: shell-session

    $ mkdir -p tests/pkg-perl-vcs/missing
    $ cd tests/pkg-perl-vcs/missing
    $ mkdir in
    $ mkdir in/debian
    $ cat <<EOF>in/debian/control
    Source: libblah-perl
    Maintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>

    Package: libblah-perl
    Description: dummy package
    EOF

    $ cp -a in out
    $ cat <<EOF>out/debian/control
    Source: libblah-perl
    Maintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>
    Vcs-Git: https://salsa.debian.org/perl-team/modules/packages/libblah-perl
    Vcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libblah-perl.git

    Package: libblah-perl
    Description: dummy package
    EOF

And finally, let's add the expected output:

.. code-block:: shell-session

    $ cat <<EOF>message
    Use standard Vcs fields for perl package.
    Certainty: certain
    Fixed-Lintian-Tags: team/pkg-perl/vcs/no-team-url
    EOF

I won't include the other tests here, but you can find them in the
`lintian-brush git repository <https://salsa.debian.org/jelmer/lintian-brush/-/tree/master/tests/pkg-perl-vcs>`_.

Writing the fixer script
~~~~~~~~~~~~~~~~~~~~~~~~

Now that the tests have been written, let's move on to the actual fixer. Each fixer
is a simple script that can also be run outside of lintian-brush.

Environment
-----------

A fixer is run in the root directory of a package, where it can make changes
it deems necessary. If a fixer can not provide any improvements, it can simply
leave the working tree untouched - lintian-brush will not create any commits for it
or update the changelog. If exits with a non-zero return code, whatever changes
it has made will be discarded and the fixer will be reported as having failed.

There is no need to interact with git - lintian-brush will make sure
fixes are run in a clean tree and takes care of updating the git index.

lintian-brush will take care of adding an entry to the changelog with the changes
that have been made, if necessary.

Output
------

Besides making changes to the package, the only thing the script needs to do is
report what changes it made on standard out, including some optional
RFC822-style pseudo-headers with other metadata.  The most common pseudoheaders
are:

* ``Fixed-Lintian-Tags``: comma-separated list of lintian tags that were fixed
  (currently just the tags, not any of the other info)
* ``Certainty``: how certain the fixer is about the changes made. Should be one
  of ``certain``, ``confident``, ``likely`` or ``possible``.

Bullet points can be used in the output, and will be appropriately formatted in
the changelog message or git commit message.

Environment variables
---------------------

Several environment variables will be set to indicate the users' preferences and
package metadate. For a list, see the section on `writing new
fixers <https://salsa.debian.org/jelmer/lintian-brush#writing-new-fixers>`_ in
the README.

Convenience functions
---------------------

If you are writing a fixer in Python, there are some convenience functions
available in the
`lintian_brush.fixer <https://salsa.debian.org/jelmer/lintian-brush/-/blob/master/lintian_brush/fixer.py>`_
module for accessing the environment
variables and reporting changes.

How the package is modified is up to the fixer. Scripts written in Python
commonly use the `debmutate <https://packages.debian.org/sid/python3-debmutate>`_
module to make changes to control files in a way that preserves formatting.

Actual script
-------------

Since our fixer script will be written in Python and the name is pkg-perl-vcs,
we'll write it in ``fixers/pkg-perl-vcs.py``:

.. code-block:: shell-session

    $ touch fixers/pkg-perl-vcs.py
    $ chmod a+x fixers/pkg-perl-vcs.py

The script itself will only need to open ``debian/control``, both
to verify that the package is maintained by the perl team and to make
any changes.

The ``ControlEditor`` context manager from ``debmutate.control`` makes this
easy. When the context is entered, the file is read. When the context is exited
without an exception, the updated file will be written back to disk if any
changes were made.

Here's what we'll do:

#. Check that the maintainer of the package is the perl team, or exit
#. Iterate over the fields in the source package that start with Vcs-

 #. If the header is Vcs-Git or Vcs-Browser:

   #. make sure that it conforms to the expected URL
   #. check that there is no override
   #. if it doesn't, verify that the expected URL exists and update the field

 #. Otherwise, remove the field

Eventually, ``fixers/pkg-perl-vcs.py`` will look something like this:

.. code-block:: python

    #!/usr/bin/python3

    import sys

    # Import convenience functions for reporting results and checking overrides
    from lintian_brush.fixer import report_result, LintianIssue

    from debmutate.control import ControlEditor
    from email.utils import parseaddr

    PKG_PERL_EMAIL = 'pkg-perl-maintainers@lists.alioth.debian.org'
    URL_BASE = 'https://salsa.debian.org/perl-team/modules/packages'

    with ControlEditor() as e:
        # Parse the maintainer field and extract the email address.
        (name, email) = parseaddr(e.source['Maintainer'])
        if email != PKG_PERL_EMAIL:
            # Nothing to do here, it's not a pkg-perl-maintained package
            sys.exit(0)
        # Iterate over all fields in the source package
        for field in list(e.source):
            if not field.lower().startswith('vcs-'):
                # Ignore non-Vcs fields
                continue
            issue = LintianIssue(e.source, 'team/pkg-perl/vcs/no-git', field)
            if field.lower() not in ('vcs-git', 'vcs-browser'):
                if not issue.should_fix():
                    continue
                # Drop this field
                del e.source[field]
                issue.report_fixed()

        for field, template in [
                ('Vcs-Git', URL_BASE + '/%s.git'),
                ('Vcs-Browser', URL_BASE + '/%s')]:
            issue = LintianIssue(e.source, 'team/pkg-perl/vcs/no-team-url', field)
            if not issue.should_fix():
                continue
            old_value = e.source.get(field)
            if old_value is not None and old_value.startswith(URL_BASE):
                continue

            e.source[field] = template % e.source['Source']
            # TODO(jelmer): Check that URLs actually exist, if net access is
            # allowed?
            issue.report_fixed()

    report_result(
        'Use standard Vcs fields for perl package.',
        certainty='certain')


Registering the fixer
~~~~~~~~~~~~~~~~~~~~~

Next, let's add the fixer to the list:

.. code-block:: shell-session

    $ cat <<EOF>>fixers/index.desc

    Fix-Script: pkg-perl-vcs.py
    Lintian-Tags: team/pkg-perl/vcs/no-team-url, team/pkg-perl/vcs/no-git
    EOF

(the name of the fixer will be the name of the script with the extension removed)

Note that the order of the list is significant and determines in what order the
fixers are applied. For this specific fixer, it's fine to be last, but you may have to
insert a fixer elsewhere in other situations.

Testing
~~~~~~~

We can now run the tests for just this fixer:

.. code-block:: shell-session

    $ make check-fixer-pkg-perl-vcs

This invokes a standard Python test runner. If there are any issues, they will
be reported to standard error. If the "after" package is different from expected,
a diff will be included.

So the fixer works okay in isolation; next, run the entire testsuite to verify
that we haven't inadvertently broken something else:

.. code-block:: shell-session

    $ make check

Book-keeping
~~~~~~~~~~~~

The fixer is verified to work, so let's commit it:

.. code-block:: shell-session

    $ dch "pkg-perl-vcs: Add fixer for team/pkg-perl/vcs/no-team-url and team/pkg-perl/vcs/no-git."
    $ git add .
    $ debcommit

... and update the list of supported tags in README.rst:

.. code-block:: shell-session

    $ make update
    ...
    $ git show
    === modified file 'README.md'
    --- old/README.md	2020-10-01 23:08:29 +0000
    +++ new/README.md	2020-10-12 16:18:39 +0000
    @@ -134,6 +134,8 @@
     * systemd-service-file-refers-to-var-run
     * systemd-service-file-shutdown-problems
     * tab-in-license-text
    +* team/pkg-perl/vcs/no-git
    +* team/pkg-perl/vcs/no-team-url
     * trailing-whitespace
     * transitional-package-not-oldlibs-optional
     * unnecessary-team-upload

Finally, verify that lintian-brush with the new fixer does the right thing on a couple of
packages. The `list of packages affected by
team/pkg-perl/vcs/no-team-url <https://lintian.debian.org/tags/team/pkg-perl/vcs/no-team-url.html>`_
is a good place to start.

.. code-block:: shell-session

    $ debcheckout libyyy-perl
    $ cd libalias-perl
    $ lintian-brush
    Lintian tags fixed: {'trailing-whitespace', 'package-uses-deprecated-debhelper-compat-version', 'vcs-field-not-canonical', 'uses-debhelper-compat-file', 'team/perl/vcs/no-team-url'}
