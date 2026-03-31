# debianize — Design Overview

This document describes the architecture and design of the `debianize` crate,
which automatically generates `debian/` directories for upstream projects.

## High-level flow

When `debianize` is invoked on an upstream source tree, it goes through these
stages:

1. **Tree preparation** — lock the working tree, verify no `debian/` directory
   exists yet (or remove it if `--force-new-directory`), and set up a
   `ResetOnFailure` guard that rolls back the tree on error.

2. **Metadata gathering** — call upstream-ontologist
   (`import_metadata_from_path()`) to extract project name, version, homepage,
   repository URL, description, license, etc. from files like `pyproject.toml`,
   `Cargo.toml`, `package.json`, and external registries (PyPI, crates.io, npm,
   etc.).

3. **Upstream version resolution** — determine which upstream version to
   package. In release mode (`--release`) this is the latest tagged release; in
   snapshot mode it is a revision-based version string.

4. **Build system detection** — call `ognibuild::buildsystem::detect_buildsystems()`
   to identify the project's build system. The first (highest-priority) match is
   used, or the user can override with `--buildsystem`.

5. **Processor dispatch** — route to a language-specific processor function
   based on the detected build system (see [Processors](#processors) below).
   The processor creates `debian/control`, `debian/rules`, and related files.

6. **Metadata finalization** — write `debian/source/format`, `debian/changelog`
   (with WNPP bug references if found), and set VCS fields (`Vcs-Git`,
   `Vcs-Browser`).

7. **Post-processing** — optionally run lintian-brush fixers to clean up policy
   issues in the generated packaging.

8. **Build and fix** (if `--iterate-fix`) — build the package in an isolated
   session and use deb-fix-build to iteratively resolve build failures.

9. **Recursive packaging** (if `--recursive`) — when a build fails due to a
   missing unpackaged dependency, clone its upstream, run `debianize`
   recursively, build it, and serve the resulting `.deb` from a local APT
   repository so the main build can continue.

## Processors

Each processor follows the same pattern: create a `debian/control` editor,
add source and binary package sections, configure debhelper, and import
dependencies from ognibuild. Processors are plain functions dispatched by
build system name — there is no shared base type or trait.

| Build system | Processor | Typical binary packages |
|---|---|---|
| setup.py / pyproject.toml | `process_setup_py` | `python3-{name}` |
| Cargo | `process_cargo` | `rust-{name}` |
| npm / node | `process_npm` | `node-{name}` |
| Maven / Gradle | `process_maven` | `lib{name}-java` |
| golang | `process_golang` | `golang-{import-path}` |
| Dist::Zilla | `process_dist_zilla` | `lib{name}-perl` |
| Module::Build::Tiny | `process_perl_build_tiny` | `lib{name}-perl` |
| R | `process_r` | `r-cran-{name}` / `r-bioc-{name}` |
| Octave | `process_octave` | `octave-{name}` |
| CMake | `process_cmake` | `{name}` |
| Make / Autotools | `process_make` | `{name}` |
| _(fallback)_ | `process_default` | `{name}` |

## Key data structures

**`DebianizePreferences`** — controls the entire debianization: trust level,
network access, build session type, compat release, verbosity, team/maintainer,
whether to run fixers, etc. Converts to `lintian_brush::FixerPreferences` for
post-processing.

**`DebianizeResult`** — returned by `debianize()`: detected VCS URL, WNPP bugs,
upstream version, created tag names and branch.

**`ProcessorContext`** (internal) — carries state through a processor call:
working tree, session, build system, metadata, upstream version, and helper
methods like `create_control_file()`, `bootstrap_debhelper()`, and
`get_project_wide_deps()`.

## Dependency stack

debianize sits on top of a number of libraries. Understanding this stack is
important both for development and for debugging (see also TROUBLESHOOTING.md):

## Build sessions

Build isolation is handled by ognibuild's session abstraction. debianize
supports three modes:

- **Plain** — no isolation, runs on the host. Fast but can pollute the system.
- **Schroot** — uses a named schroot. Requires pre-configured chroots.
- **Unshare** — user-namespace isolation with a cached Debian image. The
  default and recommended mode.

All processors and the fix-build loop operate through the `dyn Session` trait,
so the isolation method is transparent to the rest of the code.

## Iterate-fix flow (`--iterate-fix`)

After generating the initial `debian/` directory, debianize can attempt to
build the package and iteratively fix failures:

1. Build with `dpkg-buildpackage` inside the session.
2. On failure, parse the build log with buildlog-consultant.
3. Apply available fixers (missing dependencies, policy issues).
4. Commit the fix and rebuild (up to `--max-build-iterations`, default 50).

## Recursive packaging (`--recursive`)

When `--recursive` is combined with `--iterate-fix`, debianize can
automatically package missing dependencies:

1. A build failure reveals a missing dependency.
2. `DebianizeFixer` (in `fixer.rs`) looks up the upstream source for that
   dependency.
3. It clones the upstream, calls `debianize()` recursively, and builds it.
4. The resulting `.deb` is added to a `SimpleTrustedAptRepo` — a minimal HTTP
   APT repository served on localhost (in `simple_apt_repo.rs`).
5. The main build is retried with the new dependency available.

## Package naming (`names.rs`)

Upstream names are converted to Debian source/binary names using
language-specific conventions:

| Language | Example upstream | Debian name |
|---|---|---|
| Python | `foo_bar` | `python-foo-bar` / `python3-foo-bar` |
| Rust | `FooBar` | `rust-foobar` |
| Perl | `Foo::Bar` | `libfoo-bar-perl` |
| Node.js | `@scope/pkg` | `node-scope-pkg` |
| Go | `github.com/user/proj` | `golang-github-user-proj` |

## Error handling

`debianize` defines an `Error` enum covering tree state errors, missing
metadata, VCS issues, invalid package names, and wrapped errors from
underlying libraries.

A `ResetOnFailure` guard is created at the start of `debianize()`. If the
function panics or returns an error, the working tree is automatically reset
to its pre-debianize state.

Non-fatal operations (lintian fixers, VCS URL detection, WNPP bug search) log
warnings and continue rather than aborting.
