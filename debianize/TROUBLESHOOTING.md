# Troubleshooting debianize

When `debianize` produces unexpected or incomplete results, these steps can help
you understand what it sees and why it makes the choices it does.

## 1. Enable verbose output

Run debianize with `--verbose` (or `-v`) to get more detailed output about what
it is doing and why:

```sh
debianize --verbose
```

This is the first thing to try when something goes wrong — the extra output
often makes the problem obvious.

## 2. Check what ognibuild detects

Run `ogni info` in the upstream source directory to see what build system and
metadata ognibuild detects:

```sh
ogni info
```

This shows the detected build system, declared dependencies, and other
information that debianize relies on. If this output is wrong or incomplete,
debianize will produce incorrect packaging.

## 3. Check upstream metadata

Run `guess-upstream-metadata` to see what upstream metadata is detected:

```sh
guess-upstream-metadata
```

This reports fields like the project name, homepage, repository URL, bug
tracker, and description. debianize uses this metadata to populate
`debian/control`, `debian/copyright`, `debian/watch`, and other files. Missing
or incorrect metadata here explains missing or wrong fields in the generated
packaging.

You can also pass `--verbose` for more detail on where each field was found:

```sh
guess-upstream-metadata --verbose
```

## 4. Analyse build logs

If a build fails, you can use `analyse-sbuild-log` or `analyse-build-log` to
extract structured information from the build log:

```sh
analyse-sbuild-log < buildlog.txt
analyse-build-log < buildlog.txt
```

`analyse-sbuild-log` is for logs produced by sbuild; `analyse-build-log` is for
plain dpkg-buildpackage or similar logs. These tools parse the log and report
the detected error, missing dependencies, and other actionable information.
This is the same analysis that `--iterate-fix` uses internally, so running it
manually can help you understand why automatic fixing did or did not work.

## 5. Common issues

### Wrong build system detected

If `ogni info` reports the wrong build system, you can force a specific one:

```sh
debianize --buildsystem NAME
```

### Missing dependencies

If the generated `debian/control` is missing build or runtime dependencies,
check whether `ogni info` lists them. If not, the upstream metadata may be
incomplete (e.g. a missing `requirements.txt` or incomplete `Cargo.toml`
dependencies section).

### Metadata fields are wrong or missing

If `guess-upstream-metadata` does not find the correct homepage, description,
or repository URL, the upstream project may be missing standard metadata files
or conventions. You can manually edit the generated `debian/` files after
running debianize.

### Build failures

Use `--iterate-fix` / `-x` to let debianize attempt to automatically fix build
failures:

```sh
debianize --iterate-fix
```

This runs `deb-fix-build` in a loop to resolve common build issues.

## 6. Crashes and bugs

If debianize crashes or produces clearly wrong output, the bug may not be in
debianize itself. debianize builds on a stack of underlying libraries and tools,
and the issue is often in one of them:

- **ognibuild** — build system detection, dependency resolution, and build
  execution
- **upstream-ontologist** — upstream metadata guessing (used by
  `guess-upstream-metadata`)
- **lintian-brush** — Debian packaging fixers (used by `--iterate-fix`)
- **buildlog-consultant** — build log parsing (used by `analyse-build-log`,
  `analyse-sbuild-log`, and `--iterate-fix`)
- **deb822-lossless** — parsing and editing of `debian/control` and other
  Deb822-format files
- **debian-changelog** — parsing and editing of `debian/changelog`
- **debian-watch** — parsing and editing of `debian/watch`

When filing a bug, try to identify which layer is at fault. For example:

- If `guess-upstream-metadata` gives wrong results on its own, file against
  upstream-ontologist.
- If `ogni info` misdetects the build system, file against ognibuild.
- If `analyse-sbuild-log` misparses a build failure, file against
  buildlog-consultant.
- If the generated `debian/control` has syntax errors, the issue may be in
  deb822-lossless.

Including `--verbose` output and, if applicable, the build log in your bug
report makes it much easier to diagnose.
