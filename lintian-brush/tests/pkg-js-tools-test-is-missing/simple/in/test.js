test('get the gzipped size', async t => {
	t.true(await gzipSize(fixture) < fixture.length);
});
