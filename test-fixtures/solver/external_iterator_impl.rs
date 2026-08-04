struct Frame;

struct RequiresIterator<T: Iterator> {
    value: T,
}

fn check(parts: std::vec::IntoIter<Frame>) {
    let _ = RequiresIterator { value: parts };
}
