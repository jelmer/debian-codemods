# -*- Autoconf -*-
AC_PREREQ(2.59)
AC_INIT([bless], [0.6.0], [bla@example.com])
AC_CONFIG_SRCDIR([src/buffers/ByteBuffer.cs])
AM_INIT_AUTOMAKE

#
# Check for programs.
#
PKG_PROG_PKG_CONFIG

if test "x$PKG_CONFIG" = "xno"; then
	AC_MSG_ERROR([You need to install pkg-config])
fi

# C# compiler
AC_PATH_PROG(MCS, gmcs, no)

AC_OUTPUT
