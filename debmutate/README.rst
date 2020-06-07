Debmutate is a set of Python modules for manipulating the control files of
Debian packages, with the ability to preserve the existing formatting of
the control files.

It's built on top of the excellent python-debian library, and was originally
extracted from lintian-brush.

To modify one of the control files, use one of the context managers to edit the file:

    from debmutate.control import ControlMutator

    with ControlEditor(path='debian/control') as control:
        print(control.source['Maintainer'])
        control.source['Maintainer'] = "Jelmer Vernooĳ <jelmer@debian.org>"

Once you leave the context manager, the changes will be written to disk if
there were any. If the editor is unable to preserve the formatting of the
control file, it will raise a FormattingUnpreservable error. The file will be
left as-is if an exception is raised, or if the .cancel() method is called.

If the control file that was edited was generated from another control file
(e.g. debian/control.in), debmutate will attempt to update that file instead
and then regenerate debian/control. If it is unable to do so, it will raise
a GeneratedFile exception.

debmutate currently provides editors for the following control files:

 * debian/changelog
 * debian/copyright
 * debian/control
 * debian/tests/control
 * debian/watch
