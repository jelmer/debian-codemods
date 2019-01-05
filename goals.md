There are many small improvements that can be made to Debian packages
automatically. It would be great if there was an easy way for Debian developers
to apply available automatic changes.

It should be easy to add new fixers, without having to familiarize yourself
with the internals of lintian-brush. Ideally, a script written in any language
that modifies the package.

Maintainers should of course review the changes made by the tool, but they should
be able to rely on the tool making the right modifications. If the tool isn't sure
about a change (i.e. it may break the package) then it should err on the side
of caution and not make the change.

(Perhaps at some point in the future we could add a flag with the minimum
certainty of fixers to run?)

Non-goals
=========

Most things that are out of scope for lintian should be out of scope for
lintian-brush as well. In particular, updates that require changes to multiple
packages (and coordinated uploads) probably belong in a different tool.

Fixing upstream issues is out of scope, even though lintian warns about some
of those issues. Automatic fixing of these issues belongs in a
distribution-agnostic tool.
