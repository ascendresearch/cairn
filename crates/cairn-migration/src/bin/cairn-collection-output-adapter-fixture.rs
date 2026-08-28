//! Actual host child used to exercise contract-bound collection output materialization.

mod call_adapter_fixture_support;

fn main() {
    call_adapter_fixture_support::main_collection_f32_reversed();
}
