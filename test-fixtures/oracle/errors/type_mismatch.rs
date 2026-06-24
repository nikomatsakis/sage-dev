fn returns_wrong_type() -> u32 {
    "hello"
    //# ERROR
}

fn wrong_arg_type() -> u32 {
    let x: u32 = "oops";
    //# ERROR
    x
}
