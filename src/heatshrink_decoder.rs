// use alloc::vec;
// use alloc::vec::Vec;

use crate::{
    common::*, HEATSHRINK_MAX_WINDOW_BITS, HEATSHRINK_MIN_LOOKAHEAD_BITS,
    HEATSHRINK_MIN_WINDOW_BITS,
};

/// Represents a case where no bits are available.
const NO_BITS: u16 = u16::MAX;

/// Result types for decoding operations.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HSDSinkRes {
    /// Data sunk, ready to poll.
    /// Returns the number of bytes sunk
    Ok(usize),
    /// Out of space in internal buffer.
    Full,
    /// NULL argument error.
    ErrorNull,
}

/// Result types for polling operations.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HSDPollRes {
    /// Input exhausted.
    /// Returns the number of bytes output
    Empty(usize),
    /// More data remaining, call again with a fresh output buffer.
    /// Returns the number of bytes output
    More(usize),
    /// NULL arguments error.
    ErrorNull,
    /// Unknown error.
    ErrorUnknown,
}

/// Result types for finish operations.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HSDFinishRes {
    /// Output is done.
    Done,
    /// More output remains.
    More,
    /// NULL arguments error.
    ErrorNull,
}

/// States for the decoder state machine.
#[derive(Copy, Clone, Debug, PartialEq)]
enum HSDState {
    /// Tag bit state.
    TagBit,
    /// Ready to yield literal byte.
    YieldLiteral,
    /// Most significant byte of index.
    BackrefIndexMSB,
    /// Least significant byte of index.
    BackrefIndexLSB,
    /// Most significant byte of count.
    BackrefCountMSB,
    /// Least significant byte of count.
    BackrefCountLSB,
    /// Ready to yield back-reference.
    YieldBackref,
}

/// Structure representing the heatshrink decoder.
pub struct HeatshrinkDecoder {
    /// Bytes in input buffer.
    input_size: u16,
    /// Offset to the next unprocessed input byte.
    input_index: u16,
    /// Number of bytes to output.
    output_count: u16,
    /// Index for bytes to output.
    output_index: u16,
    /// Head of window buffer.
    head_index: u16,
    /// Current state machine node.
    state: HSDState,
    /// Current byte of input.
    current_byte: u8,
    /// Current bit index.
    bit_index: u8,

    /// Window buffer bits.
    window_sz2: u8,
    /// Lookahead bits.
    lookahead_sz2: u8,
    /// Input buffer size.
    input_buffer_size: u16,

    /// Input buffer, then expansion window buffer.
    buffers: Vec<u8>,
}

impl HeatshrinkDecoder {
    ///
    /// Constructs a new `HeatshrinkDecoder` with the specified buffer sizes.
    ///
    /// # Arguments
    ///
    /// * `input_buffer_size` - The size of the input buffer.
    /// * `window_sz2` - The size of the window buffer in bits.
    /// * `lookahead_sz2` - The size of the lookahead in bits.
    ///
    /// # Returns
    ///
    /// An option containing the new `HeatshrinkDecoder`, or `None` if the parameters are invalid.
    pub fn new(input_buffer_size: u16, window_sz2: u8, lookahead_sz2: u8) -> Option<Self> {
        if window_sz2 < HEATSHRINK_MIN_WINDOW_BITS
            || window_sz2 > HEATSHRINK_MAX_WINDOW_BITS
            || input_buffer_size == 0
            || lookahead_sz2 < HEATSHRINK_MIN_LOOKAHEAD_BITS
            || lookahead_sz2 >= window_sz2
        {
            return None;
        }

        let buffers_sz = (1 << window_sz2) + input_buffer_size as usize;
        Some(Self {
            input_size: 0,
            input_index: 0,
            output_count: 0,
            output_index: 0,
            head_index: 0,
            state: HSDState::TagBit,
            current_byte: 0,
            bit_index: 0,
            window_sz2,
            lookahead_sz2,
            input_buffer_size,
            buffers: vec![0; buffers_sz],
        })
    }

    ///
    /// Sinks input data into the decoder's buffer.
    ///
    /// # Arguments
    ///
    /// * `in_buf` - The input buffer containing data to sink.
    ///
    /// # Returns
    ///
    /// An `HSDSinkRes` indicating the result of the sink operation.
    /// The `Ok(usize)` variant includes the number of bytes that were successfully sunk.
    pub fn sink(&mut self, in_buf: &[u8]) -> HSDSinkRes {
        if in_buf.is_empty() {
            return HSDSinkRes::ErrorNull;
        }

        let rem = self.input_buffer_size as usize - self.input_size as usize;
        if rem == 0 {
            return HSDSinkRes::Full;
        }

        let size = rem.min(in_buf.len());
        self.buffers[self.input_size as usize..self.input_size as usize + size]
            .copy_from_slice(&in_buf[..size]);
        self.input_size += size as u16;
        HSDSinkRes::Ok(size)
    }

    ///
    /// Polls the decoder for output data.
    ///
    /// # Arguments
    ///
    /// * `out_buf` - The output buffer to fill with decompressed data.
    ///
    /// # Returns
    ///
    /// An `HSDPollRes` indicating the result of the poll operation.
    pub fn poll(&mut self, out_buf: &mut [u8]) -> HSDPollRes {
        if out_buf.is_empty() {
            return HSDPollRes::ErrorNull;
        }
        let mut output_size = 0;
        let mut oi = OutputInfo {
            buf: out_buf,
            output_size: &mut output_size,
        };

        loop {
            let in_state = self.state;
            match in_state {
                HSDState::TagBit => self.state = self.st_tag_bit(),
                HSDState::YieldLiteral => self.state = self.st_yield_literal(&mut oi),
                HSDState::BackrefIndexMSB => self.state = self.st_backref_index_msb(),
                HSDState::BackrefIndexLSB => self.state = self.st_backref_index_lsb(),
                HSDState::BackrefCountMSB => self.state = self.st_backref_count_msb(),
                HSDState::BackrefCountLSB => self.state = self.st_backref_count_lsb(),
                HSDState::YieldBackref => self.state = self.st_yield_backref(&mut oi),
            }

            if self.state == in_state {
                if *oi.output_size == oi.buf.len() {
                    return HSDPollRes::More(output_size);
                }
                return HSDPollRes::Empty(output_size);
            }
        }
    }

    /// Finishes the decoding process.
    ///
    /// Notify the dencoder that the input stream is finished.
    /// * If the return value is HSDR_FINISH_MORE, there is still more output, so
    ///* call heatshrink_decoder_poll and repeat
    ///
    /// # Returns
    ///
    /// An `HSDFinishRes` indicating whether more output remains.
    pub fn finish(&mut self) -> HSDFinishRes {
        match self.state {
            HSDState::TagBit => {
                if self.input_size == 0 {
                    HSDFinishRes::Done
                } else {
                    HSDFinishRes::More
                }
            }
            HSDState::BackrefIndexLSB
            | HSDState::BackrefIndexMSB
            | HSDState::BackrefCountLSB
            | HSDState::BackrefCountMSB => {
                if self.input_size == 0 {
                    HSDFinishRes::Done
                } else {
                    HSDFinishRes::More
                }
            }
            HSDState::YieldLiteral => {
                if self.input_size == 0 {
                    HSDFinishRes::Done
                } else {
                    HSDFinishRes::More
                }
            }
            _ => HSDFinishRes::More,
        }
    }

    /// Handles the `TagBit` state, determining whether to yield a literal or handle backreferences.
    fn st_tag_bit(&mut self) -> HSDState {
        let bits = self.get_bits(1); // get tag bit
        if bits == NO_BITS {
            HSDState::TagBit
        } else if bits != 0 {
            HSDState::YieldLiteral
        } else if self.window_sz2 > 8 {
            HSDState::BackrefIndexMSB
        } else {
            self.output_index = 0;
            HSDState::BackrefIndexLSB
        }
    }

    /// Handles the `YieldLiteral` state, emitting a literal byte to the output.
    fn st_yield_literal(&mut self, oi: &mut OutputInfo) -> HSDState {
        if *oi.output_size < oi.buf.len() {
            let byte = self.get_bits(8);
            if byte == NO_BITS {
                return HSDState::YieldLiteral;
            }
            let buf_offset = self.input_buffer_size as usize;
            let mask = (1 << self.window_sz2) - 1;
            let c = byte as u8;
            self.buffers[(self.head_index & mask) as usize + buf_offset] = c;
            self.head_index = self.head_index.wrapping_add(1);
            if *oi.output_size < oi.buf.len() {
                oi.buf[*oi.output_size] = c;
                *oi.output_size += 1;
            }
            HSDState::TagBit
        } else {
            HSDState::YieldLiteral
        }
    }

    /// Handles the `BackrefIndexMSB` state, retrieving the most significant byte of the backreference index.
    fn st_backref_index_msb(&mut self) -> HSDState {
        let bit_ct = self.window_sz2;
        assert!(bit_ct > 8);
        let bits = self.get_bits(bit_ct - 8);
        if bits == NO_BITS {
            HSDState::BackrefIndexMSB
        } else {
            self.output_index = bits << 8;
            HSDState::BackrefIndexLSB
        }
    }

    /// Handles the `BackrefIndexLSB` state, retrieving the least significant byte of the backreference index.
    fn st_backref_index_lsb(&mut self) -> HSDState {
        let bit_ct = self.window_sz2;
        let bits = self.get_bits(if bit_ct < 8 { bit_ct } else { 8 });
        if bits == NO_BITS {
            HSDState::BackrefIndexLSB
        } else {
            self.output_index |= bits;
            self.output_index += 1;
            let br_bit_ct = self.lookahead_sz2;
            self.output_count = 0;
            if br_bit_ct > 8 {
                HSDState::BackrefCountMSB
            } else {
                HSDState::BackrefCountLSB
            }
        }
    }

    /// Handles the `BackrefCountMSB` state, retrieving the most significant byte of the backreference count.
    fn st_backref_count_msb(&mut self) -> HSDState {
        let br_bit_ct = self.lookahead_sz2;
        assert!(br_bit_ct > 8);
        let bits = self.get_bits(br_bit_ct - 8);
        if bits == NO_BITS {
            HSDState::BackrefCountMSB
        } else {
            self.output_count = bits << 8;
            HSDState::BackrefCountLSB
        }
    }

    /// Handles the `BackrefCountLSB` state, retrieving the least significant byte of the backreference count.
    fn st_backref_count_lsb(&mut self) -> HSDState {
        let br_bit_ct = self.lookahead_sz2;
        let bits = self.get_bits(if br_bit_ct < 8 { br_bit_ct } else { 8 });
        if bits == NO_BITS {
            HSDState::BackrefCountLSB
        } else {
            self.output_count |= bits;
            self.output_count += 1;
            HSDState::YieldBackref
        }
    }

    /// Handles the `YieldBackref` state, emitting bytes from the backreference.
    fn st_yield_backref(&mut self, oi: &mut OutputInfo) -> HSDState {
        // Determine how much space is left in the output buffer
        let mut count = oi.buf.len() - *oi.output_size;

        // Only proceed if there is space left to write
        if count > 0 {
            // Limit the number of bytes to output_count, ensuring no overflow
            if self.output_count < count as u16 {
                count = self.output_count as usize;
            }

            // Offset in the buffer where backreference starts
            let buf_offset = self.input_buffer_size as usize;
            let buf = &mut self.buffers[buf_offset..];
            let mask = (1 << self.window_sz2) - 1;
            let neg_offset = self.output_index as usize;

            // Emitting the backreferenced bytes
            for _ in 0..count {
                // Wrap the index calculation to prevent overflow
                let index = (self.head_index as usize).wrapping_sub(neg_offset) & mask;
                let c = buf[index];

                // Push byte to output buffer
                oi.buf[*oi.output_size] = c;
                *oi.output_size += 1;

                // Add the byte to the circular buffer
                buf[self.head_index as usize & mask] = c;
                self.head_index = self.head_index.wrapping_add(1);
            }

            // Reduce the count of remaining bytes to output
            self.output_count -= count as u16;

            // If all bytes have been emitted, return to `TagBit` state
            if self.output_count == 0 {
                return HSDState::TagBit;
            }
        }
        // Remain in `YieldBackref` if there are still bytes to emit
        HSDState::YieldBackref
    }

    /// Retrieves the next `count` bits from the input buffer, saving incremental progress.
    /// Returns `NO_BITS` if end of input is reached, or if more than 15 bits are requested.
    fn get_bits(&mut self, count: u8) -> u16 {
        let mut accumulator = 0;
        if count > 15 {
            return NO_BITS;
        }

        if self.input_size == 0 && self.bit_index < (1 << (count - 1)) {
            return NO_BITS;
        }

        for _ in 0..count {
            if self.bit_index == 0x00 {
                if self.input_size == 0 {
                    return NO_BITS;
                }
                self.current_byte = self.buffers[self.input_index as usize];
                self.input_index += 1;
                if self.input_index == self.input_size {
                    self.input_index = 0;
                    self.input_size = 0;
                }
                self.bit_index = 0x80;
            }

            accumulator <<= 1;
            if self.current_byte & self.bit_index != 0 {
                accumulator |= 0x01;
            }
            self.bit_index >>= 1;
        }

        accumulator
    }
}
