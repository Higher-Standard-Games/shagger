//! Small helpers for reading BSP entity keys the way SHAG's TypeScript does.
//! Not a SHAG file: these gather the idioms `extractLifts`,
//! `extractBrushModels` and `TriggerTable` repeat inline.

use crate::js::{is_js_whitespace, js_string_to_number, js_trim};

/// An entity's classname and its key/value pairs, insertion ordered. The
/// loader lives in `shagger::loader::bsp`; this is the part the shared code needs.
pub trait EntityLike {
    fn classname(&self) -> &str;
    fn property(&self, key: &str) -> Option<&str>;
}

/// `text.trim().split(/\\s+/).map(Number)`.
#[must_use]
pub fn split_numbers(text: &str) -> Vec<f64> {
    let text = js_trim(text);
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(is_js_whitespace) {
        parts.push(js_string_to_number(&rest[..start]));
        rest = rest[start..].trim_start_matches(is_js_whitespace);
    }
    parts.push(js_string_to_number(rest));
    parts
}

/// `(origin ?? "0 0 0").trim().split(/\\s+/).map(Number)`, accepted only as
/// three finite numbers.
#[must_use]
pub fn origin_of(value: Option<&str>) -> Option<[f64; 3]> {
    let parts = split_numbers(value.unwrap_or("0 0 0"));
    (parts.len() == 3 && parts.iter().all(|v| v.is_finite())).then(|| [parts[0], parts[1], parts[2]])
}

/// `/^\\*[1-9]\\d*$/` (`positive`) or `/^\\*\\d+$/`, then `Number(model.slice(1))`
/// as an index. `None` when the pattern fails or the number is not an index.
#[must_use]
pub fn inline_model_index(model: Option<&str>, positive: bool) -> Option<usize> {
    let digits = model?.strip_prefix('*')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if positive && digits.starts_with('0') {
        return None;
    }
    let n = js_string_to_number(digits);
    // Larger values index nothing a real map holds.
    (n < 4_294_967_295.0).then(|| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = n as usize;
        n
    })
}

/// An inline model's bounds, in Q3 axes (z up).
pub trait ModelLike {
    fn mins(&self) -> [f64; 3];
    fn maxs(&self) -> [f64; 3];
}
