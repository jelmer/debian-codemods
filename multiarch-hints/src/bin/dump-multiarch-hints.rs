fn main() {
    let hints_text = multiarch_hints::download_multiarch_hints(None, None)
        .unwrap()
        .unwrap();

    let hints = multiarch_hints::parse_multiarch_hints(hints_text.as_slice()).unwrap();

    println!("{:#?}", hints);
}
