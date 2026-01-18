.DEFAULT: check

tag-status:
	cargo test --bin tag-status

update-renamed-tags:
	python3 renamed-tags.py
	brz diff renamed-tags.json || brz commit -m "Update renamed tags" renamed-tags.json

next:
	cargo run --features udd --bin tag-status -- --next

check:: testsuite

testsuite::
	cargo test
