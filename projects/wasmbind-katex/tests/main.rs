use katex_wasmbind::KaTeXOptions;
use wasm_bindgen_test::*;

#[test]
fn ready() {
    println!("it works!")
}

#[wasm_bindgen_test]
fn mode() {
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
    let d = KaTeXOptions::display_mode();
    let i = KaTeXOptions::inline_mode();
    assert_ne!(d.render("\\frac12"), i.render("\\frac12"));
}
