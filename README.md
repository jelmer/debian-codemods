# debian-codemods

This repository contains a collection of codemods (code modifications)
specifically designed for Debian packaging and development. These codemods can
help automate common tasks, improve code quality, and ensure compliance with
Debian policies.

## Available Subprojects

* [lintian-brush](lintian-brush/README.md) - A tool to automatically fix common Lintian warnings and errors in Debian packages, as reported by [lintian](https://lintian.debian.org/)
* [debianize](debianize/README.md) - Create a Debian package from scratch for an upstream source tree
* [import-uncommitted](import-uncommitted/README.md) - A tool to import previously uncommitted changes into a Git repository, e.g. missing uploads
* [multiarch-hints](multiarch-hints/README.md) - A codemod to apply [multiarch hints](https://wiki.debian.org/MultiArch/Hints) to Debian packages
* [scrub-obsolete](scrub-obsolete/README.md) - Remove obsolete entries from Debian packaging files
* [transition-apply](transition-apply/README.md) - Apply package transitions


## Related projects

* [Debian janitor](https://janitor.debian.net/) - An automated system that applies various codemods to Debian packages in an effort to improve the overall quality of the Debian archive, and then submits changes for review to the respective maintainers (directly pushing, or creating merge requests in e.g. Salsa)
* [deb822-rs](https://github.com/jelmer/deb822-rs) - Rust crates for losslessly editing various Debian control files
* [debian-analyzer](https://github.com/jelmer/debian-analyzer) - Crate to analyze and modify Debian source packages, built on top of `deb822-rs`. Provides higher level abstractions, e.g. seamless support for `debcargo` packages

## Contributing

Contributions are very welcome! The easiest way to get started is probably
by following the [guide on writing more fixers for lintian-brush](lintian-brush/doc/fixer-writing-guide.rst).

See also [CONTRIBUTING.md](CONTRIBUTING.md) on general contribution guidelines, especially
regarding code style and what belongs where.
