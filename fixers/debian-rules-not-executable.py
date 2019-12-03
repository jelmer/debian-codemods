#!/usr/bin/python3
import os

try:
    st = os.stat('debian/rules')
except FileNotFoundError:
    pass
else:
    if not (st.st_mode & 0o111):
        os.chmod('debian/rules', 0o755)


print('Make debian/rules executable.')
print('Fixed-Lintian-Tags: debian-rules-not-executable')
