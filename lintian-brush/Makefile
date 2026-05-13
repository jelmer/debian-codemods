.DEFAULT: check

tag-status:
	cargo test --bin tag-status

README.md::
	cargo run --bin tag-status -- --update-readme

update-readme:
	brz diff README.md
	$(MAKE) README.md
	brz diff README.md || brz commit -m "Update list of fixers in README.md" README.md

update-renamed-tags:
	python3 renamed-tags.py
	brz diff renamed-tags.json || brz commit -m "Update renamed tags" renamed-tags.json

next:
	cargo run --features udd --bin tag-status -- --next

check:: testsuite

testsuite::
	cargo test

update-spdx:
	python3 download-license-data.py > spdx.json
	brz diff spdx.json || brz commit -m "Update SPDX license data" spdx.json
