///!
///! This is a simple CLI that reads from stdin and writes to stdout.
///!
///! Stdin is buffered, sunk through a `HeatshrinkEncoder`, and then written to stdout.
///!
///! If the `-d` flag is passed, stdin is buffered, sunk through a `HeatshrinkDecoder`, and then written to stdout.
///!
use std::io::{self, Read, Write};
use std::process;

use tsz_heatshrink::*;

// chosen based on bar chart in 'average-compression-tsz-data.png'
const DEFAULT_WINDOW_BITS: u8 = 9;
const DEFAULT_LOOKAHEAD_BITS: u8 = 7;
const WORK_SIZE_UNIT: usize = 1024;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 2 {
        eprintln!("Usage: {} [-d]", args[0]);
        process::exit(1);
    }

    // Use stdin and stdout for I/O
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let decompress = args.len() == 2 && args[1] == "-d";
    if decompress {
        decode(&mut stdin, &mut stdout);
    } else {
        encode(&mut stdin, &mut stdout);
    }
}

#[inline]
fn read_in(stdin: &mut impl Read, buf: &mut [u8]) -> usize {
    stdin.read(buf).expect("Failed to read from stdin")
}

#[inline]
fn write_out(stdout: &mut impl Write, data: &[u8]) {
    stdout.write_all(data).expect("Failed to write to stdout");
}

/// Create an encoder, Read from stdin, Sink and Poll through the encoder, and Write polled bytes to stdout.
fn encode(stdin: &mut impl Read, stdout: &mut impl Write) {
    let mut encoder = HeatshrinkEncoder::new(DEFAULT_WINDOW_BITS, DEFAULT_LOOKAHEAD_BITS)
        .expect("Failed to create encoder");
    let mut buf = [0; WORK_SIZE_UNIT];
    let mut scratch = [0; WORK_SIZE_UNIT * 2];

    // Sink all bytes from the input buffer
    let mut not_empty = false;
    loop {
        let read_len = read_in(stdin, &mut buf);
        not_empty |= read_len > 0;
        if read_len == 0 {
            break;
        }
        let mut read_data = &buf[..read_len];
        while !read_data.is_empty() {
            let sink_res = encoder.sink(read_data);
            match sink_res {
                HSESinkRes::Ok(bytes_sunk) => {
                    read_data = &read_data[bytes_sunk..];
                }
                _ => unreachable!(),
            }

            loop {
                match encoder.poll(&mut scratch) {
                    HSEPollRes::Empty(sz) => {
                        write_out(stdout, &scratch[..sz]);
                        break;
                    }
                    HSEPollRes::More(sz) => {
                        write_out(stdout, &scratch[..sz]);
                    }
                    HSEPollRes::ErrorMisuse | HSEPollRes::ErrorNull => unreachable!(),
                }
            }
        }
    }

    if !not_empty {
        return;
    }

    // Poll out the remaining bytes
    loop {
        match encoder.finish() {
            HSEFinishRes::Done => {
                break;
            }
            HSEFinishRes::More => {}
            HSEFinishRes::ErrorNull => unreachable!(),
        }

        loop {
            match encoder.poll(&mut scratch) {
                HSEPollRes::Empty(sz) => {
                    write_out(stdout, &scratch[..sz]);
                    break;
                }
                HSEPollRes::More(sz) => {
                    write_out(stdout, &scratch[..sz]);
                }
                HSEPollRes::ErrorMisuse | HSEPollRes::ErrorNull => unreachable!(),
            }
        }
    }
}

/// Create a decoder, Read from stdin, Sink and Poll through the decoder, and Write polled bytes to stdout.
fn decode(stdin: &mut impl Read, stdout: &mut impl Write) {
    let mut decoder = HeatshrinkDecoder::new(
        WORK_SIZE_UNIT as u16,
        DEFAULT_WINDOW_BITS,
        DEFAULT_LOOKAHEAD_BITS,
    )
    .expect("Failed to create decoder");
    let mut buf = [0; WORK_SIZE_UNIT];
    let mut scratch = [0; WORK_SIZE_UNIT * 2];

    // Sink all bytes from the input buffer
    let mut not_empty = false;
    loop {
        let read_len = read_in(stdin, &mut buf);
        not_empty |= read_len > 0;
        if read_len == 0 {
            break;
        }
        let mut read_data = &buf[..read_len];
        while !read_data.is_empty() {
            let sink_res = decoder.sink(read_data);
            match sink_res {
                HSDSinkRes::Ok(bytes_sunk) => {
                    read_data = &read_data[bytes_sunk..];
                }
                _ => unreachable!(),
            }

            loop {
                match decoder.poll(&mut scratch) {
                    HSDPollRes::Empty(sz) => {
                        write_out(stdout, &scratch[..sz]);
                        break;
                    }
                    HSDPollRes::More(sz) => {
                        write_out(stdout, &scratch[..sz]);
                    }
                    HSDPollRes::ErrorNull => unreachable!(),
                    HSDPollRes::ErrorUnknown => {
                        eprintln!("Error: Unknown");
                        process::exit(-1);
                    }
                }
            }
        }
    }

    if !not_empty {
        return;
    }

    // Poll out the remaining bytes
    loop {
        match decoder.finish() {
            HSDFinishRes::Done => {
                break;
            }
            HSDFinishRes::More => {}
            HSDFinishRes::ErrorNull => unreachable!(),
        }

        loop {
            match decoder.poll(&mut scratch) {
                HSDPollRes::Empty(sz) => {
                    write_out(stdout, &scratch[..sz]);
                    break;
                }
                HSDPollRes::More(sz) => {
                    write_out(stdout, &scratch[..sz]);
                }
                HSDPollRes::ErrorNull => unreachable!(),
                HSDPollRes::ErrorUnknown => {
                    eprintln!("Error: Unknown");
                    process::exit(-1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_pass_fuzz_fail_0() {
        for i in 0..=1024 {
            let zeros = vec![0u8; i];
            let mut compressed = vec![];
            encode(&mut zeros.as_slice(), &mut compressed);

            let mut decompressed = vec![];
            decode(&mut compressed.as_slice(), &mut decompressed);

            assert_eq!(zeros, decompressed, "Failed at i = {}", i);
        }
    }
}
