//! Smoke lane for the Node-API formula entries.
//!
//! Calling any of these entries used to abort the host process: the math core
//! panicked when no embedded face was installed, and the panic crossed the
//! foreign boundary. This lane proves both halves of the fix. A host with no
//! face gets a diagnostic, and every entry installs the faces once and returns
//! a payload. Reaching the end of the test is itself the survival criterion.

use serde_json::Value;

fn parse(label: &str, raw: String) -> Value {
    let value: Value = serde_json::from_str(&raw).unwrap_or_else(|error| panic!("{label} returned invalid JSON: {error}"));
    assert!(value.is_object(), "{label} must return an object");
    value
}

fn has_code(value: &Value, code: &str) -> bool {
    value["diagnostics"].as_array().is_some_and(|items| items.iter().any(|item| item["code"] == code))
}

#[test]
fn formula_entries_report_then_install_and_return_payloads() {
    // Before any entry runs, the core has no installed face. It must report a
    // diagnostic instead of unwinding through the export boundary.
    let missing = watex_wasm::measure("x^2", &Default::default());
    assert!(
        missing.diagnostics.iter().any(|item| item.code == "WATEX_FONT_UNAVAILABLE"),
        "a host without an embedded face must report WATEX_FONT_UNAVAILABLE"
    );

    let entries: [(&str, fn(String, Option<String>) -> String); 4] = [
        ("measureFormula", vutex_napi::measure_formula),
        ("renderFormulaSvg", vutex_napi::render_formula_svg),
        ("renderFormulaMathml", vutex_napi::render_formula_mathml),
        ("renderFormula", vutex_napi::render_formula),
    ];

    for (label, entry) in entries {
        let value = parse(label, entry("x^2".to_string(), None));
        assert!(!has_code(&value, "WATEX_FONT_UNAVAILABLE"), "{label} must install the embedded faces");
        assert!(!has_code(&value, "WATEX_MATH_ERROR"), "{label} must accept `x^2`");
        let width = value["metrics"]["width"].as_f64().unwrap_or(0.0);
        assert!(width > 0.0, "{label} must report a positive width, got {width}");
    }

    println!("four formula entries returned a payload and the process is alive");
}
