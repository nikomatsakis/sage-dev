struct Frame;

enum ParseError {
    EndOfStream,
}

trait Fallback {
    fn ok_or(self, error: ParseError) -> Result<Frame, ParseError>;
}

impl Fallback for Option<Frame> {
    fn ok_or(self, _error: ParseError) -> Result<Frame, ParseError> {
        loop {}
    }
}

fn next_item(mut parts: std::vec::IntoIter<Frame>) -> Result<Frame, ParseError> {
    parts.next().ok_or(ParseError::EndOfStream)
}

fn never_error(value: Option<Frame>) -> Result<Frame, bool> {
    value.ok_or(loop {})
}
