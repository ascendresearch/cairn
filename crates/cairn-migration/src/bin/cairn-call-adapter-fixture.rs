//! Deterministic zero-output host fixture for the isolated call-adapter protocol.

#[path = "call_adapter_fixture_support/mod.rs"]
mod support;

fn main() {
    support::main_with_output_byte(0);
}
