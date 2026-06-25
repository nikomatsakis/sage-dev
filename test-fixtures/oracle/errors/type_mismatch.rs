fn returns_wrong_type() -> u32 { //# ERROR type mismatch
    "hello"
}

fn wrong_arg_type() -> u32 { //# ERROR type mismatch
    let x: u32 = "oops";
    x
}
