In addition to various bits of metadata that it can automatically work out are
obsolete, deb-scrub-obsolete will (in a future version) also allow annotations
in control files that developers can use to indicate when entries can be removed.

These annotations take the form of comments with a particular format; when the condition
is met, the line, block or file that it is included in will be removed. What the comment looks
like depends on the file format.

Annotations can start with a marker name, which is optional and can not include
any spaces or commas or be the word "after". deb-scrub-obsolete will take a
list of markers that can be removed on the command-line.

When parsing these expressions, we'll have to be liberal in what to accept so long as it is
unambiguous. This is because most of these lines will have been written without
formal verification. It might make sense for deb-scrub-obsolete to provide an option to
validate the syntax of "# scrub" commands, perhaps allowing for bugs to be filed for
incorrect entries.

If any scrub comments can not be interpreted, none of them are removed. This is so that
if there are related blocks that need to be removed together, we don't end up removing just one if
the other has an invalid expression.

# Supported file formats

## Shell files

### Single line

For shell files, the comments take the format ``# scrub: [<marker-name>, ][after <expression>]`` after a line,
indicating that the line can be removed altogether when the expression is true. For example,
``# scrub: released:trixie`` means that the line can be removed when trixie has
been released.

Comments can appear anywhere in the line, i.e. all of these are recognized:

```shell
blah  # scrub: after released(trixie) # Trixie comes with blah built in
blah  # scrub: after trixie
blah  # scrub: blah-transition, after released(trixie)
blah  # scrub: blah-transition
blah  # Trixie comes with blah built in # after scrub(released(trixie)
```

### Block

Alternatively, entire blocks of code can be selected using the following syntax:

```shell

# begin scrub: released(trixie)
alternatives --add foo bar
alternatives --add foo bar1
# end scrub

```

These sections can be nested.

# Supported expressions

Ideally expressions can only transition from false to true, since otherwise the implication is that
a particular section should be brought back or would not be removed if deb-scrub-obsolete were to run
later.

The following expressions will initially be supported:

* ``released:$name`` if the Debian release with specified codename is released. E.g. ``released:trixie``
* ``in($suite): $package >= $version`` - if a particular suite has at least version $version of $package. E.g. ``in(unstable): systemd >= 3.5``
* ``in($suite): !$package`` - if a particular suite no longer has a package
* ``supported: $package >= $version`` - if all supported versions have at least verison $version of $package. E.g. ``supported: systemd >= 3.5``
* ``transition:$name`` if a particular transition on https://release.debian.org/transitions/ has been marked as done. E.g. ``transition:fuse-to-fuse3``

For package version comparisons, only ``>=`` and ``>>`` are supported, since
they are the only conditions that will still hold true in future releases. If a package is removed
from the archive, its latest version that was present in the archive is used.

Otherwise, the following are attempted one by one to interpret the expression, until one is valid:

* ``$name`` is an alias for ``released:$name`` if $name is a known Debian release name
* ``$name`` is an alias for ``transition:$name`` if $name is a known transition name
* ``$package (>= $version)`` is an alias for ``in($current\_suite): $package (>= $version)``, where ``$current_suite`` is the suite the package is being built for
