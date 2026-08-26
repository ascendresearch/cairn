//! Deterministic one-output host fixture used as a deliberately wrong admission variant.

#[path = "call_adapter_fixture_support/mod.rs"]
mod support;

fn main() {
    support::main_with_output_byte(1);
}
