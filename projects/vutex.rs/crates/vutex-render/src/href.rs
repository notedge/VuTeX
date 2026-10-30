//! Destination safety shared by every backend that emits anchors.

/// Whether a run destination may be written into an `href` attribute.
///
/// Relative references, fragments, and the listed schemes are emitted. Anything
/// else, including script-capable schemes such as `javascript:`, is dropped so
/// the text still renders but no misleading or executable destination appears.
/// An empty destination is never emitted.
pub fn is_safe_href(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let Some((scheme, _)) = value.split_once(':')
    else {
        return true;
    };
    if scheme.contains('/') || scheme.contains('?') || scheme.contains('#') {
        return true;
    }
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "mailto" | "tel")
}
