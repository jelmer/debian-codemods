#!/usr/bin/python3

from lintian_brush.reformatting import check_generated_file

check_generated_file('debian/rules')

with open('debian/rules', 'rb') as f:
    oldcontents = f.read()

newcontents = oldcontents.replace(b'$(PWD)', b'$(CURDIR)')

if oldcontents != newcontents:
    with open('debian/rules', 'wb') as f:
        f.write(newcontents)

print("debian/rules: Avoid using $(PWD) variable.")
print("Fixed-Lintian-Tags: debian-rules-should-not-use-pwd")
