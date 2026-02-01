See lintian-brush/CONTRIBUTING.md for linux-brush-specific contribution guidelines.

## What belongs where?

Anything that just purely deals purely with file formats, etc. should probably got into
one of the crates specific to that (e.g. in [deb822-rs](https://github.com/jelmer/deb822-rs)).

General tooling to do with modifying packages, etc. should go into
[debian-analyzer](https://github.com/jelmer/debian-analyzer).

Things that fix lintian tags should go into [lintian-brush](lintian-brush). See
also [lintian-brush' CONTRIBUTING.md](lintian-brush/CONTRIBUTING.md) and
[lintian-brush' goals](lintian-brush/goals.md).
