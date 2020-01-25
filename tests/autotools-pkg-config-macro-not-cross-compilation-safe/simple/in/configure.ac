# -*- Autoconf -*-
AC_PREREQ(2.59)
AC_INIT([bless], [0.6.0], [bla@example.com])
AC_CONFIG_SRCDIR([src/buffers/ByteBuffer.cs])
AM_INIT_AUTOMAKE

#
# Check for programs.
#
AC_PATH_PROG(PKG_CONFIG,pkg-config, no)

if test "x$PKG_CONFIG" = "xno"; then
	AC_MSG_ERROR([You need to install pkg-config])
fi

# C# compiler
AC_PATH_PROG(MCS, gmcs, no)

AC_OUTPUT
