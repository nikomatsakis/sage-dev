#![allow(warnings)]

enum Message {
    Data { value: u32 },
}

fn make_message(v: u32) -> Message {
    Message::Data { value: v }
}
