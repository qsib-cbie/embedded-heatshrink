// Heatshrink internal constants
pub(crate) const HEATSHRINK_LITERAL_MARKER: u8 = 1;
pub(crate) const HEATSHRINK_BACKREF_MARKER: u8 = 0;

// Heatshrink internal types
#[derive(Debug)]
pub(crate) struct OutputInfo<'a> {
    /// output buffer
    pub buf: &'a mut [u8],
    /// bytes pushed to the buffer, so far
    pub output_size: &'a mut usize,
}

#[inline]
#[cold]
fn cold() {}

#[inline]
pub(crate) fn unlikely(b: bool) -> bool {
    if b {
        cold()
    }
    b
}
