fn ok_val(x: u32) -> Result<u32, u32> {
    Result::Ok(x)
}

fn err_val(x: u32) -> Result<u32, u32> {
    Result::Err(x)
}
