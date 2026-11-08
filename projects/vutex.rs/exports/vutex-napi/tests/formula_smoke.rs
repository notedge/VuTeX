//! Smoke lane for the Node-API formula entries.
//!
//! Calling any of these entries used to abort the host process: the math core
//! panicked when no face was installed, and the panic crossed the foreign
//! boundary. The cut-over removed the embedded pack, so the contract now has
//! two halves. A host with no configured assets gets a diagnostic from both the
//! WASM core and the native entries, and once it configures the repository pack
//! every entry returns a payload. Reaching the end of the test is itself the
//! survival criterion.

use serde_json::Value;

fn parse(label: &str, raw: String) -> Value {
    let value: Value = serde_json::from_str(&raw).unwrap_or_else(|error| panic!("{label} returned invalid JSON: {error}"));
    assert!(value.is_object(), "{label} must return an object");
    value
}

fn has_code(value: &Value, code: &str) -> bool {
    value["diagnostics"].as_array().is_some_and(|items| items.iter().any(|item| item["code"] == code))
}

fn configure_fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts");
        vutex_napi::configure_font_assets_dir(fonts.display().to_string(), None).expect("configure repository font pack");
    });
}

#[test]
fn formula_entries_report_then_configure_and_return_payloads() {
    // Neither the WASM core nor the native entry may invent a default face.
    let missing = watex_wasm::measure("x^2", &Default::default());
    assert!(
        missing.diagnostics.iter().any(|item| item.code == "VUTEX_FONT_ASSETS_NOT_CONFIGURED"),
        "the WASM core must report VUTEX_FONT_ASSETS_NOT_CONFIGURED"
    );
    let native_missing = parse("measureFormula", vutex_napi::measure_formula("x^2".to_string(), None));
    assert!(
        has_code(&native_missing, "VUTEX_FONT_ASSETS_NOT_CONFIGURED"),
        "the native entry must report VUTEX_FONT_ASSETS_NOT_CONFIGURED before configuration"
    );

    configure_fonts();

    let entries: [(&str, fn(String, Option<String>) -> String); 4] = [
        ("measureFormula", vutex_napi::measure_formula),
        ("renderFormulaSvg", vutex_napi::render_formula_svg),
        ("renderFormulaMathml", vutex_napi::render_formula_mathml),
        ("renderFormula", vutex_napi::render_formula),
    ];

    for (label, entry) in entries {
        let value = parse(label, entry("x^2".to_string(), None));
        assert!(!has_code(&value, "VUTEX_FONT_ASSETS_NOT_CONFIGURED"), "{label} must use the configured faces");
        assert!(!has_code(&value, "WATEX_MATH_ERROR"), "{label} must accept `x^2`");
        let width = value["metrics"]["width"].as_f64().unwrap_or(0.0);
        assert!(width > 0.0, "{label} must report a positive width, got {width}");
    }

    println!("four formula entries returned a payload and the process is alive");
}
