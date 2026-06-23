#![feature(rustc_private)]

use std::sync::Arc;

use libtest_mimic::{Arguments, Failed, Trial};
use sage_oracle_harness::{
    Fixture, assert_crates_eq, discover_fixtures, normalize_pair, strip_bodies,
};

fn run_signatures(fixture: &Fixture) -> Result<(), Failed> {
    let oracle = strip_bodies(&fixture.oracle_output());
    let sage = strip_bodies(&fixture.sage_output());
    assert_crates_eq(&format!("{} [signatures]", fixture.name()), &oracle, &sage)
        .map_err(Failed::from)
}

fn run_full(fixture: &Fixture) -> Result<(), Failed> {
    let oracle_raw = fixture.oracle_output();
    let sage_raw = fixture.sage_output();
    let (oracle, sage) = normalize_pair(&oracle_raw, &sage_raw);
    assert_crates_eq(&format!("{} [full]", fixture.name()), &oracle, &sage).map_err(Failed::from)
}

fn main() {
    let args = Arguments::from_args();
    let fixtures: Vec<Arc<Fixture>> = discover_fixtures().into_iter().map(Arc::new).collect();

    let mut tests = Vec::new();
    for fixture in &fixtures {
        let name = fixture.name();

        let f = Arc::clone(fixture);
        tests.push(Trial::test(format!("{name}::signatures"), move || {
            run_signatures(&f)
        }));

        let f = Arc::clone(fixture);
        tests.push(Trial::test(format!("{name}::full"), move || run_full(&f)));
    }

    libtest_mimic::run(&args, tests).exit();
}
