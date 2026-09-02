struct Frame;

fn takes_vec(value: Vec<Frame>) {
    let _ = value;
}

fn push_never(mut value: Vec<Frame>) {
    value.push(loop {});
}
