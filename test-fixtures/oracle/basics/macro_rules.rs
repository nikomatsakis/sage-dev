macro_rules! make_getter {
    () => {
        fn get_value() -> u32 {
            42
        }
    };
}

make_getter!();

fn use_getter() -> u32 {
    get_value()
}
