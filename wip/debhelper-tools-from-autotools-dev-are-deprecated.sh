#!/bin/sh
# Basically:
#  * remove "--with autotools-dev" in debian/rules
#  * replace all calls to dh_autotools-dev_updateconfig with dh_update_autotools_config
#  * calls to dh_autotools-dev_restoreconfig can just be removed
