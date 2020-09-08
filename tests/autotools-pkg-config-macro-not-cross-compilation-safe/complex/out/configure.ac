# -*- Autoconf -*-
AC_PREREQ(2.59)
AC_INIT([bless], [0.6.0], [bla@example.com])
AC_CONFIG_SRCDIR([src/buffers/ByteBuffer.cs])
AM_INIT_AUTOMAKE

#
# Check for programs.
#

AC_PATH_TOOL([PKGCONFIG], [pkg-config], [false], [$PATH$PATH_SEPARATOR$with_pkg_config/bin])
AS_IF([test x$PKGCONFIG = xfalse], [AC_MSG_ERROR([You don't have pkg-config installed])])

# C# compiler
AC_PATH_PROG(MCS, gmcs, no)

AC_OUTPUT
