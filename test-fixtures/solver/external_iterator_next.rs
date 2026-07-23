struct Frame;

fn next_item(mut parts: std::vec::IntoIter<Frame>) -> Option<Frame> {
    parts.next()
}
