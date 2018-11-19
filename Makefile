check:: style testsuite

.PHONY: style testsuite unsupported

style::
	flake8

testsuite::
	python3 setup.py test

README.md::
	PYTHONPATH=. ./buildtools/update-readme.py

unsupported:
	lintian-info --list-tags > lintian-tags
	PYTHONPATH=. python3 -m lintian_brush --list-tags 2> lintian-brush-tags
	awk 'NR==FNR{a[$$0]=1;next}!a[$$0]' lintian-brush-tags lintian-tags
