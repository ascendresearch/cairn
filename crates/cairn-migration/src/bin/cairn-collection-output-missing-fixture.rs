//! Fault implementation that drops one selected occurrence after a real adapter invocation.

mod call_adapter_fixture_support;

fn main() {
    call_adapter_fixture_support::main_collection_f32_missing_occurrence();
}
