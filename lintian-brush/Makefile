.DEFAULT: check

tag-status:
	PYTHONPATH=$(shell pwd)/../py python3 tag-status.py --check

update-renamed-tags:
	python3 lintian-brush/renamed-tags.py
	brz diff lintian-brush/renamed-tags.json || brz commit -m "Update renamed tags" lintian-brush/renamed-tags.json

next:
	PYTHONPATH=$(shell pwd)/../py python3 next.py

check:: testsuite

testsuite::
	PYTHONPATH=$(shell pwd)/../py cargo test 
