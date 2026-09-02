fn result() -> u32 {
    loop {}
}

fn takes(_: u32) -> u32 {
    0
}

fn argument() -> u32 {
    takes(loop {})
}

struct Holder {
    value: u32,
}

fn field() -> Holder {
    Holder { value: loop {} }
}
