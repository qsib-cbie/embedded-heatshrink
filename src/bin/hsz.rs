///!
///! This is a simple CLI that reads from stdin and writes to stdout.
///!
///! Stdin is buffered, sunk through a `HeatshrinkEncoder`, and then written to stdout.
///!
///! If the `-d` flag is passed, stdin is buffered, sunk through a `HeatshrinkDecoder`, and then written to stdout.
///!
use std::io::{self};
use std::process;

use embedded_heatshrink::*;

// chosen based on bar chart in 'average-compression-tsz-data.png'
const DEFAULT_WINDOW_BITS: u8 = 9;
const DEFAULT_LOOKAHEAD_BITS: u8 = 7;

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
        decode(
            DEFAULT_WINDOW_BITS,
            DEFAULT_LOOKAHEAD_BITS,
            &mut stdin,
            &mut stdout,
        );
    } else {
        encode(
            DEFAULT_WINDOW_BITS,
            DEFAULT_LOOKAHEAD_BITS,
            &mut stdin,
            &mut stdout,
        );
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
            encode(
                DEFAULT_WINDOW_BITS,
                DEFAULT_LOOKAHEAD_BITS,
                &mut zeros.as_slice(),
                &mut compressed,
            );

            let mut decompressed = vec![];
            decode(
                DEFAULT_WINDOW_BITS,
                DEFAULT_LOOKAHEAD_BITS,
                &mut compressed.as_slice(),
                &mut decompressed,
            );

            assert_eq!(zeros, decompressed, "Failed at i = {}", i);
        }
    }
}
