// use alloc::vec;
// use alloc::vec::Vec;
use core::cmp::min;
use core::ptr;

use crate::{
    common::*, HEATSHRINK_MAX_WINDOW_BITS, HEATSHRINK_MIN_LOOKAHEAD_BITS,
    HEATSHRINK_MIN_WINDOW_BITS,
};

// Define result types for encoding operations
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HSESinkRes {
    /// data sunk into input buffer
    /// returns the number of bytes actually sunk
    Ok(usize),
    /// NULL argument
    ErrorNull,
    /// misuse of API
    ErrorMisuse,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HSEPollRes {
    /// input exhausted
    /// returns the number of bytes actually copied
    Empty(usize),
    /// poll again for more output
    /// returns the number of bytes actually copied
    More(usize),
    /// NULL argument
    ErrorNull,
    /// misuse of API
    ErrorMisuse,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HSEFinishRes {
    /// encoding is completed
    Done,
    /// more output remaining; use poll
    More,
    /// NULL argument
    ErrorNull,
}

// Define the states for the encoder state machine
#[derive(Copy, Clone, Debug, PartialEq)]
enum HSEState {
    /// input buffer not full enough
    NotFull,
    /// buffer is full
    Filled,
    /// searching for patterns
    Search,
    /// yield tag bit
    YieldTagBit,
    /// emit literal byte
    YieldLiteral,
    /// yielding backref index
    YieldBrIndex,
    /// yielding backref length
    YieldBrLength,
    /// copying buffer to backlog
    SaveBacklog,
    /// flush bit buffer
    FlushBits,
    /// done
    Done,
}

// Define constants for match not found
const MATCH_NOT_FOUND: u16 = u16::MAX;

pub struct HeatshrinkEncoder {
    /// bytes in input buffer
    input_size: usize,
    match_scan_index: usize,
    match_length: usize,
    match_pos: u16,
    /// enqueued outgoing bits
    outgoing_bits: u16,
    outgoing_bits_count: u8,
    flags: u8,
    /// current state machine node
    state: HSEState,
    /// current byte of output
    current_byte: u8,
    /// current bit index
    bit_index: u8,
    /// 2^n size of window
    window_sz2: u8,
    /// 2^n size of lookahead
    lookahead_sz2: u8,
    /// size of window and input buffer
    input_buffer_size: usize,
    /// size of lookahead
    lookahead_size: usize,
    /// search index
    /// using dynamic allocation
    search_index: Vec<i16>,
    /// input buffer and / sliding window for expansion
    /// using dynamic allocation
    buffer: Vec<u8>,
}

impl HeatshrinkEncoder {
    ///
    ///  Initialize the `HeatshrinkEncoder` with:
    ///    * 1<<window_sz2 byte window for the current input
    ///    * 1<<window_sz2 byte window for the previous input, for backreferences
    ///    * 1<<lookahead_sz2 byte lookahead
    ///
    /// ```rust
    /// use tsz_heatshrink::HeatshrinkEncoder;
    /// let mut encoder = HeatshrinkEncoder::new(8, 4).expect("Failed to create encoder");
    /// ```
    pub fn new(window_sz2: u8, lookahead_sz2: u8) -> Option<Self> {
        if window_sz2 < HEATSHRINK_MIN_WINDOW_BITS
            || window_sz2 > HEATSHRINK_MAX_WINDOW_BITS
            || lookahead_sz2 < HEATSHRINK_MIN_LOOKAHEAD_BITS
            || lookahead_sz2 >= window_sz2
        {
            return None;
        }

        // the buffer needs to fit the 1 << window_sz2 bytes for the current input and
        // the 1 << window_sz2 bytes for the previous input, which will be scanned
        // for useful backreferences.
        let buf_sz = (2 << window_sz2) as usize;

        Some(HeatshrinkEncoder {
            input_size: 0,
            match_scan_index: 0,
            match_length: 0,
            match_pos: 0,
            outgoing_bits: 0,
            outgoing_bits_count: 0,
            flags: 0,
            state: HSEState::NotFull,
            current_byte: 0,
            bit_index: 0x80,
            window_sz2,
            lookahead_sz2,
            input_buffer_size: 1 << window_sz2,
            lookahead_size: 1 << lookahead_sz2,
            search_index: vec![0; buf_sz],
            buffer: vec![0; buf_sz],
        })
    }

    ///
    /// Sink all of the bytes in in_buf to the encoder, if bytes must be emitted
    /// they are emitted to out_buf. The number of bytes actually emitted is returned.
    ///
    /// If the return value is HSER_POLL_MORE, then the out_buf is full and the
    /// number of bytes sunk from in_buf are returned.
    /// Otherwise, HSER_POLL_EMPTY is returned with the number of bytes emitted to out_buf.
    #[inline]
    pub fn sink_all(&mut self, in_buf: &[u8], out_buf: &mut [u8]) -> HSEPollRes {
        if out_buf.is_empty() {
            return HSEPollRes::ErrorMisuse;
        }

        let mut sunk = 0;
        let mut polled = 0;
        while sunk < in_buf.len() {
            match self.sink(in_buf) {
                HSESinkRes::Ok(sz) => {
                    sunk += sz;
                }
                _ => {
                    return HSEPollRes::ErrorMisuse;
                }
            }

            loop {
                if polled == out_buf.len() {
                    return HSEPollRes::More(sunk);
                }
                match self.poll(&mut out_buf[polled..]) {
                    HSEPollRes::Empty(sz) => {
                        polled += sz;
                        break;
                    }
                    HSEPollRes::More(sz) => {
                        polled += sz;
                    }
                    e => unreachable!("Logic error: {:?}", e),
                }
            }
        }

        HSEPollRes::Empty(sunk)
    }

    ///
    /// Sink up to `in_buf.len()` bytes from `in_buf` into the encoder.
    /// the number of bytes actually sunk is returned on success.
    ///
    /// Do not provide an empty in_buf
    #[inline]
    pub fn sink(&mut self, in_buf: &[u8]) -> HSESinkRes {
        // TODO: remove these checks and improve the docs instead
        // These checks are in the hot loop of pushing data through the encoder
        // this function gets called roughly O(n / 100) times for O(n) bytes of input
        if (self.is_finishing()) | (self.state != HSEState::NotFull) {
            return HSESinkRes::ErrorMisuse;
        }

        // Calculate the offset and remaining bytes at the end of the input buffer window
        let write_offset = self.get_input_offset() + self.input_size;
        let rem = self.input_buffer_size - self.input_size;
        let cp_sz = min(rem, in_buf.len()); // 0 if full

        // Copy as many bytes as possible into the input buffer
        self.buffer[write_offset..write_offset + cp_sz].copy_from_slice(&in_buf[..cp_sz]);
        self.input_size += cp_sz;

        // If the input buffer is full, then caller needs to poll to progress
        if cp_sz == rem {
            self.state = HSEState::Filled;
        }

        HSESinkRes::Ok(cp_sz)
    }

    /// Poll for output from the encoder, copying at most `out_buf.len()` bytes
    /// into `out_buf`. The number of bytes actually copied is returned on success.
    ///
    /// Do not provide an empty out_buf
    ///
    #[inline]
    pub fn poll(&mut self, out_buf: &mut [u8]) -> HSEPollRes {
        // Looping through states will fill the output buffer, accumulating the output size
        let mut output_size = 0;
        let mut oi = OutputInfo {
            buf: out_buf,
            output_size: &mut output_size,
        };
        loop {
            let in_state = self.state;
            self.state = match in_state {
                HSEState::Done | HSEState::NotFull => return HSEPollRes::Empty(output_size),
                HSEState::Filled => {
                    self.do_indexing();
                    HSEState::Search
                }
                HSEState::Search => self.st_step_search(),
                HSEState::YieldTagBit => self.st_yield_tag_bit(&mut oi),
                HSEState::YieldLiteral => self.st_yield_literal(&mut oi),
                HSEState::YieldBrIndex => self.st_yield_br_index(&mut oi),
                HSEState::YieldBrLength => self.st_yield_br_length(&mut oi),
                HSEState::SaveBacklog => self.st_save_backlog(),
                HSEState::FlushBits => self.st_flush_bit_buffer(&mut oi),
            };

            if self.state == in_state {
                if *oi.output_size == oi.buf.len() {
                    return HSEPollRes::More(output_size);
                }
            }
        }
    }

    /// Notify the encoder that the input stream is finished.
    /// If the return value is HSER_FINISH_MORE, there is more output to poll, so
    /// call poll until it returns HSER_FINISH_DONE.
    pub fn finish(&mut self) -> HSEFinishRes {
        self.flags |= FLAG_IS_FINISHING;
        if self.state == HSEState::NotFull {
            // Mark the input filled to trigger indexing and emission of the remaining data
            self.state = HSEState::Filled;
        }
        if self.state == HSEState::Done {
            HSEFinishRes::Done
        } else {
            HSEFinishRes::More
        }
    }

    #[inline]
    fn st_step_search(&mut self) -> HSEState {
        let window_length = self.input_buffer_size;
        let lookahead_sz = self.lookahead_size;
        let msi = self.match_scan_index;

        let fin = self.is_finishing();
        if msi > self.input_size - (if fin { 1 } else { lookahead_sz }) {
            return if fin {
                HSEState::FlushBits
            } else {
                HSEState::SaveBacklog
            };
        }

        let input_offset = self.get_input_offset();
        let end = input_offset + msi;
        let start = end - window_length;

        let mut max_possible = lookahead_sz;
        if self.input_size - msi < lookahead_sz {
            max_possible = self.input_size - msi;
        }

        let mut match_length = 0;
        let match_pos = self.find_longest_match(start, end, max_possible, &mut match_length);

        if match_pos == MATCH_NOT_FOUND {
            self.match_scan_index += 1;
            self.match_length = 0;
            HSEState::YieldTagBit
        } else {
            self.match_pos = match_pos;
            self.match_length = match_length;
            debug_assert!(match_pos <= 1 << self.window_sz2); // matching within window size
            HSEState::YieldTagBit
        }
    }

    #[inline]
    fn st_yield_tag_bit(&mut self, oi: &mut OutputInfo) -> HSEState {
        if self.can_take_byte(oi) {
            if self.match_length == 0 {
                self.add_tag_bit(oi, HEATSHRINK_LITERAL_MARKER);
                HSEState::YieldLiteral
            } else {
                self.add_tag_bit(oi, HEATSHRINK_BACKREF_MARKER);
                self.outgoing_bits = self.match_pos - 1;
                self.outgoing_bits_count = self.get_window_bits();
                HSEState::YieldBrIndex
            }
        } else {
            HSEState::YieldTagBit
        }
    }

    #[inline]
    fn st_yield_literal(&mut self, oi: &mut OutputInfo) -> HSEState {
        if self.can_take_byte(oi) {
            self.push_literal_byte(oi);
            HSEState::Search
        } else {
            HSEState::YieldLiteral
        }
    }

    #[inline]
    fn st_yield_br_index(&mut self, oi: &mut OutputInfo) -> HSEState {
        if self.can_take_byte(oi) {
            if self.push_outgoing_bits(oi) > 0 {
                HSEState::YieldBrIndex // continue
            } else {
                self.outgoing_bits = (self.match_length - 1) as u16;
                self.outgoing_bits_count = self.get_lookahead_bits();
                HSEState::YieldBrLength // done
            }
        } else {
            HSEState::YieldBrIndex // continue
        }
    }

    #[inline]
    fn st_yield_br_length(&mut self, oi: &mut OutputInfo) -> HSEState {
        if self.can_take_byte(oi) {
            if self.push_outgoing_bits(oi) > 0 {
                HSEState::YieldBrLength
            } else {
                self.match_scan_index += self.match_length;
                self.match_length = 0;
                HSEState::Search
            }
        } else {
            HSEState::YieldBrLength
        }
    }

    #[inline]
    fn st_save_backlog(&mut self) -> HSEState {
        self.save_backlog();
        HSEState::NotFull
    }

    #[inline]
    fn st_flush_bit_buffer(&mut self, oi: &mut OutputInfo) -> HSEState {
        if self.bit_index == 0x80 {
            HSEState::Done
        } else if self.can_take_byte(oi) {
            oi.buf[*oi.output_size] = self.current_byte;
            *oi.output_size += 1;
            HSEState::Done
        } else {
            HSEState::FlushBits
        }
    }

    #[inline]
    fn add_tag_bit(&mut self, oi: &mut OutputInfo, tag: u8) {
        self.push_bits(1, tag, oi);
    }

    #[inline]
    fn get_input_offset(&self) -> usize {
        self.input_buffer_size
    }

    #[inline]
    fn do_indexing(&mut self) {
        const FILL: i16 = -1;
        let mut last: [i16; 256] = [FILL; 256];

        let data = &self.buffer;
        let input_offset = self.get_input_offset();
        let index = &mut self.search_index;
        let end = input_offset + self.input_size;
        for i in 0..end {
            let v = data[i] as usize;
            index[i] = last[v];
            last[v] = i as i16;
        }
    }

    #[inline]
    fn is_finishing(&self) -> bool {
        self.flags & FLAG_IS_FINISHING == FLAG_IS_FINISHING
    }

    #[inline]
    fn can_take_byte(&self, oi: &OutputInfo) -> bool {
        *oi.output_size < oi.buf.len()
    }

    #[inline]
    fn find_longest_match(
        &self,
        start: usize,
        end: usize,
        maxlen: usize,
        match_length: &mut usize,
    ) -> u16 {
        let buf = &self.buffer;

        let mut match_maxlen = 0;
        let mut match_index = MATCH_NOT_FOUND;

        let needlepoint = &buf[end..];
        let hsi = &self.search_index;
        let mut pos = hsi[end];
        let break_even_point =
            ((1 + self.get_window_bits() + self.get_lookahead_bits()) / 8) as usize;
        while pos - (start as i16) >= 0 {
            if pos < 0 {
                // Write to stderr
                eprintln!(
                    "window_sz2: {}, lookahead_sz2: {}, start: {}, end: {}, maxlen: {}, pos: {} start: {}",
                    self.window_sz2, self.lookahead_sz2,
                    start, end, maxlen, pos, start
                );
            }
            let posidx = pos as usize;
            let pospoint = &buf[posidx..];

            if pospoint[match_maxlen] != needlepoint[match_maxlen] {
                pos = hsi[posidx];
                continue;
            }

            let mut len = 1;
            while len < maxlen {
                if pospoint[len] != needlepoint[len] {
                    break;
                }
                len += 1;
            }

            if len > match_maxlen {
                match_maxlen = len;
                match_index = pos as u16;
                if len == maxlen {
                    break;
                }
            }
            pos = hsi[posidx];
        }

        if match_maxlen > break_even_point {
            *match_length = match_maxlen;
            end as u16 - match_index
        } else {
            MATCH_NOT_FOUND
        }
    }

    #[inline]
    fn push_outgoing_bits(&mut self, oi: &mut OutputInfo) -> u8 {
        let count: u8;
        let bits: u8;
        if self.outgoing_bits_count > 8 {
            count = 8;
            bits = (self.outgoing_bits >> (self.outgoing_bits_count - 8)) as u8;
        } else {
            count = self.outgoing_bits_count;
            bits = self.outgoing_bits as u8;
        }

        if count > 0 {
            self.push_bits(count, bits, oi);
            self.outgoing_bits_count -= count;
        }
        count
    }

    #[inline]
    fn push_bits(&mut self, count: u8, bits: u8, oi: &mut OutputInfo) {
        debug_assert!(count <= 8);

        if count == 8 && self.bit_index == 0x80 {
            oi.buf[*oi.output_size] = bits;
            *oi.output_size += 1;
        } else {
            for i in (0..count).rev() {
                let bit = bits & (1 << i) != 0;
                if bit {
                    self.current_byte |= self.bit_index;
                }
                self.bit_index >>= 1;
                if self.bit_index == 0x00 {
                    self.bit_index = 0x80;
                    oi.buf[*oi.output_size] = self.current_byte;
                    *oi.output_size += 1;
                    self.current_byte = 0x00;
                }
            }
        }
    }

    #[inline]
    fn push_literal_byte(&mut self, oi: &mut OutputInfo) {
        let processed_offset = self.match_scan_index - 1;
        let input_offset = self.get_input_offset() + processed_offset;
        let c = self.buffer[input_offset];
        self.push_bits(8, c, oi);
    }

    #[inline]
    fn save_backlog(&mut self) {
        let rem = self.input_buffer_size - self.match_scan_index;
        let shift_sz = self.input_buffer_size + rem;

        unsafe {
            ptr::copy(
                self.buffer.as_ptr().add(self.input_buffer_size - rem),
                self.buffer.as_mut_ptr(),
                shift_sz,
            );
        }

        self.match_scan_index = 0;
        self.input_size -= self.input_buffer_size - rem;
    }

    ///
    /// Get the number of bits describing the window size,
    /// where 2^n is the size of the current window and previous window.
    ///
    #[inline]
    fn get_window_bits(&self) -> u8 {
        self.window_sz2
    }

    ///
    /// Get the number of bits describing the lookahead size,
    /// where 2^n is the size of the lookahead buffer.
    ///
    #[inline]
    fn get_lookahead_bits(&self) -> u8 {
        self.lookahead_sz2
    }
}

const FLAG_IS_FINISHING: u8 = 0x01;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity() {
        let mut encoder = HeatshrinkEncoder::new(8, 4).expect("Failed to create encoder");
        let input_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let sink_res = encoder.sink(&input_data);
        println!("Sink result: {:?}", sink_res);

        let mut output_buffer: Vec<u8> = vec![0; 32];
        let mut written = 0;
        let poll_res = encoder.poll(&mut output_buffer);
        println!("Poll result: {:?}", poll_res);
        match poll_res {
            HSEPollRes::Empty(sz) | HSEPollRes::More(sz) => {
                written += sz;
            }
            _ => {}
        }

        let mut finish_res = encoder.finish();
        println!("Finish result: {:?}", finish_res);
        while finish_res == HSEFinishRes::More {
            let poll_res = encoder.poll(&mut output_buffer);
            println!("Poll result: {:?}", poll_res);
            match poll_res {
                HSEPollRes::Empty(sz) | HSEPollRes::More(sz) => {
                    written += sz;
                }
                _ => {}
            }

            finish_res = encoder.finish();
            println!("Finish result: {:?}", finish_res);
        }

        println!(
            "Wrote {} bytes: {:2X?}",
            written,
            output_buffer[..written].to_vec()
        );
    }
}
