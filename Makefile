check:: style testsuite

style::
	flake8

testsuite::
	python3 setup.py test

README.md::
	./buildtools/update-readme.py
