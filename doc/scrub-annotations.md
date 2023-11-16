In addition to various bits of metadata that it can automatically work out are
obsolete, deb-scrub-obsolete will (in a future version) also allow annotations
in control files that developers can use to indicate when entries can be removed.

These annotations take the form of comments with a particular format; when the condition
is met, the line, block or file that it is included in will be removed. What the comment looks
like depends on the file format.

# Supported file formats

## Shell files

For shell files, the comments take the format ``# scrub: <condition>`` after a line,
indicating that the line can be removed altogether when the condition is met. For example,
``# scrub: released(trixie)`` means that the line can be removed when trixie has
been released.

Comments can appear anywhere in the line, i.e. all of these are recognized:

```shell
blah  # scrub: released(trixie): After trixie, we won't need to call blah manually anymore.
blah  # After trixie we won't need to call blah manually anymore # scrub(released(trixie)
```

# Supported conditions

The following conditions will initially be supported:

* ``released($name)`` if the Debian release with specified codename is released
* ``testing($name)`` if the Debian release with specified codename is current testing
* ``stable($name)`` if the Debian release with specified codenamse is current stable
