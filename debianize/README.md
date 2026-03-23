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

To contribute to debianize you need to create a fork for the [upstream](https://salsa.debian.org/jelmer/debian-codemods.git) and create a new branch to carry out your development under the `/debianize` directory. 

**Creating a dev environment**

Since **debianize** is still in its early stages, you will need to make a testing environment to carry out our changes, so that it won't effect your actual distro. For that you might a need a container or a VM to test your changes. 

To streamline your development process with debianize, you'll need some libraries and tools to help you out.

- ognibuild
    `cargo install ognibuild`
- guess-upstream-metadata
    `cargo install guess-upstream-metadata`
- breezy
    `apt install brz`
- brz-debian (debian plugin for breezy)
    `apt install brz-debian`

>Note: For the apt based libraries, you might need to add unstable repositores to your `/etc/apt/sources.list.d` if you're not using any container or VM, so that you will get latest versions of the libraries.

