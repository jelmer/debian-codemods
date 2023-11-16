In addition to various bits of metadata that it can automatically work out are
obsolete, deb-scrub-obsolete will (in a future version) also allow annotations
in control files that developers can use to indicate when entries can be removed.

These annotations take the form of comments with a particular format; when the condition
is met, the line, block or file that it is included in will be removed. What the comment looks
like depends on the file format.

Annotations can start with a marker name, which is optional and can not include
any spaces or commas. deb-scrub-obsolete will take a list of markers that can be removed.

When parsing these expressions, we'll have to be liberal in what to accept so long as it is
unambiguous. This is because most of these lines will have been written without
formal verification. It might make sense for deb-scrub-obsolete to provide an option to
validate the syntax of "# scrub" commands, perhaps allowing for bugs to be filed for
incorrect entries.

# Supported file formats

## Shell files

### Single line

For shell files, the comments take the format ``# scrub: [<marker-name>, ][after <condition>]`` after a line,
indicating that the line can be removed altogether when the condition is met. For example,
``# scrub: released(trixie)`` means that the line can be removed when trixie has
been released.

Comments can appear anywhere in the line, i.e. all of these are recognized:

```shell
blah  # scrub: after released(trixie): Trixie comes with blah built in
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

# Supported conditions

The following conditions will initially be supported:

* ``released($name)`` if the Debian release with specified codename is released. E.g. ``released(trixie)``
* ``testing($name)`` if the Debian release with specified codename is current testing. E.g. ``testing(trixie)``
* ``stable($name)`` if the Debian release with specified codename is current stable. E.g.  ``stable(trixie)``
* ``has($suite, $condition)`` - if a particular apt archive meets a condition. E.g. ``has(unstable, systemd >= 3.5)``
* ``transition(webkit2gtk)`` if a particular transition on https://release.debian.org/transitions/ has been marked as done. E.g. ``transition(fuse-to-fuse3)``
