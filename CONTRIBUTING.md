Contributing
============

Philosophy
----------

The fixers in lintian-brush should be as simple as possible. They don't have to
deal with version control, and can just give up and have their changes reverted
for them (by exiting with a non-zero exit code).

Fixers should be as fast as possible when they do not find anything to fix, since
this is the common case.

Coding Style
------------

lintian-brush uses PEP8 as its coding style.

Code style can be checked by running ``flake8``:

```shell
flake8
```

Tests
-----

To run the testsuite, use:

```shell
python3 -m unittest lintian_brush.tests.test_suite
```

or simply:

```shell
make check
```

To run the tests for a specific-fixer, run something like:

```shell
make check-fixer-ancient-maintscript-entry
```

The tests are also run by the package build and autopkgtest.
