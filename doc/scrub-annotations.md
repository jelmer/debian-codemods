In addition to various bits of metadata that it can automatically work out are
obsolete, deb-scrub-obsolete will (in a future version) also allow annotations
in control files that developers can use to indicate when entries can be
removed.

These annotations take the form of comments with a particular format; when the
condition is met, the line, block or file that it is included in will be
removed. What the comment looks like depends on the file format.

Annotations can start with a marker name, which is optional and can not include
any spaces or commas or be the word "after". deb-scrub-obsolete will take a
list of markers that can be removed on the command-line.

When parsing these expressions, we'll have to be liberal in what to accept so
long as it is unambiguous. This is because most of these lines will have been
written without formal verification. It might make sense for deb-scrub-obsolete
to provide an option to validate the syntax of "# remove-after" commands,
perhaps allowing for bugs to be filed for incorrect entries.

If any remove-after comments can not be interpreted, none of them are removed.
This is so that if there are related blocks that need to be removed together,
we don't end up removing just one if the other has an invalid expression.

# Supported file formats

## Shell files

### Single line

For shell files, the comments take the format ``# remove-after: [<marker-name>,
][after <expression>]`` after a line, indicating that the line can be removed
altogether when the expression is true. For example,
``# remove-after: released:trixie`` means that the line can be removed when
trixie has been released.

Comments can appear anywhere in the line, i.e. all of these are recognized:

```shell
blah  # remove-after: trixie # Trixie comes with blah built in
blah  # remove-after: trixie
blah  # Trixie comes with blah built in # remove-after: trixie
```

### Block

Alternatively, entire blocks of code can be selected using the following syntax:

```shell

# begin-remove-after: trixie
alternatives --add foo bar
alternatives --add foo bar1
# end-remove-after

```

These sections can be nested.

# Supported expressions

Ideally expressions can only transition from false to true, since otherwise the
implication is that a particular section should be brought back or would not be
removed if deb-scrub-obsolete were to run later.

The following expressions will initially be supported:

* ``$name`` if $name is a known Debian release name, checks for whether that
  release is out.
                                                                    0
# Future extensions

In the future, we might consider other expression that check things such as:

* whether a particular suite has a new enough version of a package
* whether a package has been removed in a suite
* whether all supported releases contain a new enough version of a package
* whether a particular transition
  (as listed on https://release.debian.org/transitions/) has completed

as well as more complicated expressions, e.g. combining expressions with "and"
or "or".
