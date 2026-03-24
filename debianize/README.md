# debianize

Create Debian packaging from upstream sources.

`debianize` automatically generates a `debian/` directory for upstream projects,
detecting the build system, extracting metadata, and producing standard Debian
packaging files (`debian/control`, `debian/changelog`, `debian/rules`, etc.).

It supports a wide range of build systems and languages, including Python
(setup.py/pyproject.toml), Rust (Cargo), Node.js (npm), Java (Maven), Go,
Perl, CMake, Autotools, and more.

## Status

**Experimental** — generated packaging is often incomplete and may require
manual adjustments before it is fully buildable.

## Usage

```sh
# Debianize the current directory
debianize

# Debianize from a specific upstream branch
debianize --upstream https://github.com/example/project

# Package the latest release rather than a snapshot
debianize --release

# Build in an isolated environment
debianize --session unshare

# Iteratively fix build failures
debianize --iterate-fix

# Recursively package missing dependencies
debianize --recursive
```

### Key options

| Option | Description |
|--------|-------------|
| `--directory PATH` | Target directory (default: current) |
| `--upstream URL` | Upstream branch location |
| `--release` | Package latest release instead of snapshot |
| `--upstream-version VERSION` | Specify upstream version explicitly |
| `--session [plain\|schroot\|unshare]` | Build isolation type |
| `--trust` | Allow running code from the package |
| `--iterate-fix` / `-x` | Run deb-fix-build to iteratively fix build issues |
| `--install` / `-i` | Build and install the package |
| `--recursive` / `-r` | Package missing dependencies too |
| `--team EMAIL` | Set maintainer team |
| `--buildsystem NAME` | Force a specific build system |

## Library usage

The crate can also be used as a library:

```rust
use debianize::{debianize, DebianizePreferences};
```

The main entry point is the `debianize()` function, which takes a working tree,
preferences, upstream metadata, and an upstream branch, and returns a
`DebianizeResult`.


## Contributing to debianize

To contribute to debianize you need to create a fork for the [upstream](https://salsa.debian.org/jelmer/debian-codemods.git) and create a new branch, to carry out your development under the `/debianize` directory. 

**Creating a dev environment**

To add or test features for **debianize**, it is recommended to develop on an unstable system - whether on bare metal or in a VM/container. For carrying your development process with debianize, you'll need some libraries and tools to help you out.

- additional dependencies assuming you're working on an unstable system<br>
    `apt build-dep debian-codemods`
- _ognibuild_ (helps in packaging)<br>
    `cargo install ognibuild`
- _upstream-ontologist_ (helps in creating the metadata for the package)<br>
    `cargo install upstream-ontologist`

>Note: Although not **recommended** if you're using a stable system, you might need to add unstable repositories to your `/etc/apt/sources.list.d` if you're not following the above setup, so that you will get latest versions of the dependencies.

