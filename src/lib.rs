//!
//! This is a fairly faithful port of the heatshrink library to Rust.
//!
//! The streaming C-style API for encoding and decoding is implemented
//! with the `heatshrink_encoder` and `heatshrink_decoder` modules. The
//! heatshrink index assuming dynamic allocation is implemented is present
//! under the assumption that this library may be used in a no_std context
//! with alloc support.
//!
// #![cfg_attr(not(test), no_std)]
// #![cfg(not(test))]
// extern crate alloc;

// How do I prevent the formatter from moving the comments to the top of the file?
// I want the comments to be at the top of the file, but the formatter keeps moving them to the bottom of the file.

pub(crate) mod common;
pub mod heatshrink_decoder;
pub mod heatshrink_encoder;
pub mod io;
pub use heatshrink_decoder::*;
pub use heatshrink_encoder::*;

/// Heatshrink constant limits
pub const HEATSHRINK_MIN_WINDOW_BITS: u8 = 4;
pub const HEATSHRINK_MAX_WINDOW_BITS: u8 = 14; // 15; // I notice some issues with 15 window_sz2: 15, lookahead_sz2: 5, start: 2, end: 32770, maxlen: 5, pos: -32767 start: 2 thread '<unnamed>' panicked at src/heatshrink_encoder.rs:425:32: range start index 18446744073709518849 out of range for slice of length 65536
pub const HEATSHRINK_MIN_LOOKAHEAD_BITS: u8 = 3;

///
/// One-shot stream encode the input into a finished compressed buffer.
///
pub fn encode_all(input: &[u8], window_sz2: u8, lookahead_sz2: u8, read_sz: usize) -> Vec<u8> {
    let mut encoder =
        HeatshrinkEncoder::new(window_sz2, lookahead_sz2).expect("Failed to create encoder");
    let mut compressed = vec![];
    let mut scratch: Vec<u8> = vec![0; read_sz * 2];
    let mut read_offset = 0;

    // Sink all bytes from the input buffer
    while read_offset < input.len() {
        let read_len = if input.len() - read_offset > read_sz {
            read_sz
        } else {
            input.len() - read_offset
        };
        let mut read_data = &input[read_offset..read_offset + read_len];
        while !read_data.is_empty() {
            let sink_res = encoder.sink(read_data);
            match sink_res {
                HSESinkRes::Ok(bytes_sunk) => {
                    read_data = &read_data[bytes_sunk..];
                }
                _ => panic!("Failed to sink data: {:?}", sink_res),
            }

            loop {
                match encoder.poll(&mut scratch) {
                    HSEPollRes::Empty(sz) => {
                        compressed.extend(&scratch[..sz]);
                        break;
                    }
                    HSEPollRes::More(sz) => {
                        compressed.extend(&scratch[..sz]);
                    }
                    e => panic!("Failed to poll data: {:?}", e),
                }
            }
        }

        read_offset += read_len;
    }

    // Poll out the remaining bytes
    loop {
        match encoder.finish() {
            HSEFinishRes::Done => {
                break;
            }
            _ => {}
        }

        loop {
            match encoder.poll(&mut scratch) {
                HSEPollRes::Empty(sz) => {
                    compressed.extend(&scratch[..sz]);
                    break;
                }
                HSEPollRes::More(sz) => {
                    compressed.extend(&scratch[..sz]);
                }
                e => panic!("Failed to poll data: {:?}", e),
            }
        }
    }

    compressed
}

pub fn decode_all(
    input: &[u8],
    input_buffer_size: usize,
    window_sz2: u8,
    lookahead_sz2: u8,
    read_sz: usize,
) -> Vec<u8> {
    let mut decoder = HeatshrinkDecoder::new(input_buffer_size as u16, window_sz2, lookahead_sz2)
        .expect("Failed to create decoder");
    let mut decompressed = vec![];
    let mut scratch: Vec<u8> = vec![0; read_sz * 2];
    let mut read_offset = 0;

    // Sink all bytes from the input buffer
    while read_offset < input.len() {
        let read_len = if input.len() - read_offset > read_sz {
            read_sz
        } else {
            input.len() - read_offset
        };
        let mut read_data = &input[read_offset..read_offset + read_len];
        while !read_data.is_empty() {
            let sink_res = decoder.sink(read_data);
            match sink_res {
                HSDSinkRes::Ok(bytes_sunk) => {
                    read_data = &read_data[bytes_sunk..];
                }
                _ => panic!("Failed to sink data: {:?}", sink_res),
            }

            loop {
                match decoder.poll(&mut scratch) {
                    HSDPollRes::Empty(sz) => {
                        decompressed.extend(&scratch[..sz]);
                        break;
                    }
                    HSDPollRes::More(sz) => {
                        decompressed.extend(&scratch[..sz]);
                    }
                    e => panic!("Failed to poll data: {:?}", e),
                }
            }
        }

        read_offset += read_len;
    }

    // Poll out the remaining bytes
    loop {
        match decoder.finish() {
            HSDFinishRes::Done => {
                break;
            }
            _ => {}
        }

        loop {
            match decoder.poll(&mut scratch) {
                HSDPollRes::Empty(sz) => {
                    decompressed.extend(&scratch[..sz]);
                    break;
                }
                HSDPollRes::More(sz) => {
                    decompressed.extend(&scratch[..sz]);
                }
                e => panic!("Failed to poll data: {:?}", e),
            }
        }
    }

    decompressed
}

#[cfg(test)]
mod tests {
    use rayon::prelude::*;
    use std::time::Instant;

    use super::*;

    fn roundtrip(
        input: &[u8],
        window_sz2: u8,
        lookahead_sz2: u8,
        in_read_sz: usize,
        out_read_sz: usize,
        out_buffer_sz: usize,
    ) -> (Vec<u8>, Vec<u8>) {
        let compressed = encode_all(input, window_sz2, lookahead_sz2, in_read_sz);
        let decompressed = decode_all(
            &compressed,
            out_buffer_sz,
            window_sz2,
            lookahead_sz2,
            out_read_sz,
        );
        (compressed, decompressed)
    }

    #[test]
    fn end2end_sanity_mock() {
        let input_data: Vec<u8> = (0..100).flat_map(|x| vec![x; 10]).collect();
        println!(
            "Input {} bytes: {:02X?}",
            input_data.len(),
            input_data.as_slice()
        );

        // Encode
        let compressed = encode_all(&input_data, 8, 4, 16);

        println!(
            "Wrote {} bytes: {:02X?}",
            compressed.len(),
            compressed.as_slice()
        );

        // Decode
        let decompressed = decode_all(&compressed, 100, 8, 4, 16);

        println!(
            "Read {} bytes: {:02X?}",
            decompressed.len(),
            decompressed.as_slice()
        );

        // Check
        for i in 0..input_data.len() {
            if i >= decompressed.len() {
                assert_eq!(input_data[i], 0, "{}: {} == {}", i, input_data[i], "EOF");
                continue;
            }
            assert_eq!(
                input_data[i], decompressed[i],
                "{}: {} == {}",
                i, input_data[i], decompressed[i]
            );
        }
    }

    /// Configuration used to track the compression configurations
    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)] // used by Debug
    struct RoundtripConfig {
        window_sz2: u8,
        lookahead_sz2: u8,
        in_read_sz: usize,
        out_read_sz: usize,
        out_buffer_sz: usize,
        file_name: &'static str,
        compressed_size: usize,
        compression_ratio: f32,
        compression_time_us: usize,
    }

    #[test]
    fn end2end_sanity_param_sweep() {
        // Compress several different types of files from B to KB to MB
        let simple_text_data = include_bytes!("../test.txt");
        let text_data = include_bytes!("heatshrink_encoder.rs");
        let random_medium_size_data = include_bytes!("../random-data.bin");
        let real_medium_size_data = include_bytes!("../tsz-compressed-data.bin");
        let data: Vec<(&'static str, &[u8])> = vec![
            ("text.txt", simple_text_data),
            ("heatshrink_encoder.rs", text_data),
            ("random-data.bin", random_medium_size_data),
            ("tsz-compressed-data.bin", real_medium_size_data),
        ];

        // Use all possible window and lookahead sizes
        let window_lookahead_pairs = (HEATSHRINK_MIN_WINDOW_BITS..=HEATSHRINK_MAX_WINDOW_BITS)
            .flat_map(|window_sz2| {
                (HEATSHRINK_MIN_LOOKAHEAD_BITS..window_sz2)
                    .map(move |lookahead_sz2| (window_sz2, lookahead_sz2))
            });

        // Use several different read and buffer sizes
        let read_buffer_sizes = [1, 2, 8, 64, 512, 4096];
        let read_size_pairs = read_buffer_sizes
            .iter()
            .flat_map(|&read_sz| {
                read_buffer_sizes
                    .iter()
                    .map(move |&buf_sz| (read_sz, buf_sz))
            })
            .collect::<Vec<_>>();

        // Use several different input buffer sizes to stress different code paths
        let input_buffer_sizes = [1, 64, 512, 8192];

        // // Run the permutations, tracking the best and worst compression ratios and times
        // let mut results = vec![];
        // for (window_sz2, lookahead_sz2) in window_lookahead_pairs {
        //     for (in_read_sz, out_read_sz) in read_size_pairs.iter() {
        //         for out_buffer_sz in input_buffer_sizes.iter() {
        //             for data in data.iter() {
        //                 let t0 = Instant::now();
        //                 let (compressed, decompressed) = roundtrip(
        //                     data.1,
        //                     window_sz2,
        //                     lookahead_sz2,
        //                     *in_read_sz,
        //                     *out_read_sz,
        //                     *out_buffer_sz,
        //                 );
        //                 let t1 = Instant::now();
        //                 let elapsed = t1 - t0;
        //                 let elapsed_us = elapsed.as_micros();
        //                 let compression_ratio = compressed.len() as f32 / data.1.len() as f32;
        //                 let config = RoundtripConfig {
        //                     window_sz2,
        //                     lookahead_sz2,
        //                     in_read_sz: *in_read_sz,
        //                     out_read_sz: *out_read_sz,
        //                     out_buffer_sz: *out_buffer_sz,
        //                     file_name: data.0,
        //                     compressed_size: compressed.len(),
        //                     compression_ratio,
        //                     compression_time_us: elapsed_us as usize,
        //                 };
        //                 println!("{:?}", config);
        //                 assert_eq!(data.1, decompressed.as_slice());
        //                 results.push(config);
        //             }
        //         }
        //     }
        // }

        // Use rayon to run all the permutations in parallel
        let mut configurations = vec![];
        for (window_sz2, lookahead_sz2) in window_lookahead_pairs {
            for (in_read_sz, out_read_sz) in read_size_pairs.iter() {
                for out_buffer_sz in input_buffer_sizes.iter() {
                    for data in data.iter() {
                        configurations.push((
                            window_sz2,
                            lookahead_sz2,
                            *in_read_sz,
                            *out_read_sz,
                            *out_buffer_sz,
                            data,
                        ));
                    }
                }
            }
        }

        println!("Running {} configurations", configurations.len());
        let t0 = Instant::now();

        let mut results: Vec<RoundtripConfig> = configurations
            .into_par_iter()
            .map(
                |(window_sz2, lookahead_sz2, in_read_sz, out_read_sz, out_buffer_sz, data)| {
                    // Run the roundtrip configuration several times to get an average
                    let mut compression_ratio = 0.0;
                    let mut elapsed_us = 0;
                    let mut compressed_len = 0;
                    const ITERS: usize = 5;
                    for i in 0..ITERS {
                        let t0 = Instant::now();
                        let (compressed, decompressed) = roundtrip(
                            data.1,
                            window_sz2,
                            lookahead_sz2,
                            in_read_sz,
                            out_read_sz,
                            out_buffer_sz,
                        );
                        let t1 = Instant::now();
                        let elapsed = t1 - t0;
                        elapsed_us += elapsed.as_micros();
                        compression_ratio = data.1.len() as f32 / compressed.len() as f32;
                        if i == 0 {
                            compressed_len = compressed.len();
                        }
                        assert_eq!(compressed_len, compressed.len());
                        assert_eq!(data.1, decompressed.as_slice());
                    }
                    let config = RoundtripConfig {
                        window_sz2,
                        lookahead_sz2,
                        in_read_sz,
                        out_read_sz,
                        out_buffer_sz,
                        file_name: data.0,
                        compressed_size: compressed_len,
                        compression_ratio,
                        compression_time_us: elapsed_us as usize / ITERS,
                    };
                    println!("{:?}", config);
                    config
                },
            )
            .collect();

        // Print top 3 and bottom 3 compression ratios
        results.sort_by(|a, b| {
            a.compression_ratio
                .partial_cmp(&b.compression_ratio)
                .unwrap()
        });
        println!("Bottom 3 compression ratios:");
        for i in 0..3 {
            println!("{:?}", results[i]);
        }
        println!("Top 3 compression ratios:");
        for i in (results.len() - 3)..results.len() {
            println!("{:?}", results[i]);
        }

        // Print top 3 and bottom 3 compression times
        results.sort_by(|a, b| {
            a.compression_time_us
                .partial_cmp(&b.compression_time_us)
                .unwrap()
        });
        println!("Top 3 compression times:");
        for i in 0..3 {
            println!("{:?}", results[i]);
        }
        println!("Bottom 3 compression times:");
        for i in (results.len() - 3)..results.len() {
            println!("{:?}", results[i]);
        }

        let t1 = Instant::now();
        println!("Completed permutations in {:?}", t1 - t0);
    }
}
