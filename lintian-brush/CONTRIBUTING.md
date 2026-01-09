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

lintian-brush fixers are written in Rust.

Code style can be checked by running ``cargo clippy``:

```shell
cargo clippy
```

Code can be formatted with ``cargo fmt``:

```shell
cargo fmt
```

Tests
-----

To run the testsuite, use:

```shell
make check
```

To run the tests for a specific-fixer, run something like:

```shell
cargo test fixer_name
```

(with any dashes in the fixer name replaced by underscores).

The tests are also run by the package build and autopkgtest.
