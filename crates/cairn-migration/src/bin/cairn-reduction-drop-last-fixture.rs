mod reduction_fixture_support;

fn main() {
    reduction_fixture_support::main_for(cairn_migration::HistoricalReductionAlgorithm::DropLast);
}
