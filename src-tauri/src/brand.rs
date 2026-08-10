/// Brand symbol — single source of truth for product name and identifiers.
/// Mirrors `brand.json` at the repository root. Values are duplicated here
/// as `&'static str` constants so Rust code does not need runtime JSON parsing.
/// Keep in sync with `brand.json` and `src/brand.mjs`.
pub const PRODUCT_NAME: &str = "Orthic";
pub const BUNDLE_IDENTIFIER: &str = "com.orthic.hub";
pub const IDENTIFIER: &str = "com.orthic.hub";
pub const DMG_NAME_TEMPLATE: &str = "Orthic_${version}_aarch64.dmg";
