struct Frame;

enum ParseError {
    EndOfStream,
}

struct Parse {
    parts: std::vec::IntoIter<Frame>,
}

impl Parse {
    fn next(&mut self) -> Result<Frame, ParseError> {
        self.parts.next().ok_or(ParseError::EndOfStream)
    }
}
