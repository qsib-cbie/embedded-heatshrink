#!/bin/bash

# Files to test
FILES=("tsz-compressed-data.bin" "fuzz/index.html" "average-compression-tsz-data.png")

# Compression tools and their commands
tools=("gzip" "bzip2" "xz" "zstd" "hsz")

get_tool_command() {
    local tool=$1
    case $tool in
        "gzip") echo "gzip -c" ;;
        "bzip2") echo "bzip2 -c" ;;
        "xz") echo "xz -c" ;;
        "zstd") echo "zstd -c" ;;
        "hsz") echo "hsz" ;;
        *) echo "" ;;
    esac
}

# Function to measure time and output size
measure() {
    local tool=$1
    local command=$(get_tool_command $tool)
    local infile=$2
    local outfile="$infile.$tool"
    local decompressfile="$outfile.dec"

    # Initialize sums for averaging
    local sum_compression_time=0
    local sum_decompression_time=0
    local sum_compression_ratio=0

    # Number of runs for averaging
    local runs=5

    for ((i=0; i<runs; i++)); do
        if [ "$tool" == "hsz" ]; then
            # Use a pipe for hsz compression and decompression
            gtime -f "%e" sh -c "$command < '$infile' > '$outfile'" 2> compress_time.txt
            compress_time=$(<compress_time.txt)
            compressed_size=$(stat -f%z "$outfile")

            gtime -f "%e" sh -c "$command -d < '$outfile' > '$decompressfile'" 2> decompress_time.txt
            decompress_time=$(<decompress_time.txt)
        else
            # Standard compression and decompression
            gtime -f "%e" sh -c "$command '$infile' > '$outfile'" 2> compress_time.txt
            compress_time=$(<compress_time.txt)
            compressed_size=$(stat -f%z "$outfile")

            gtime -f "%e" sh -c "$command -d '$outfile' > '$decompressfile'" 2> decompress_time.txt
            decompress_time=$(<decompress_time.txt)
        fi

        # Get original file size
        original_size=$(stat -f%z "$infile")

        # Calculate compression ratio
        compression_ratio=$(echo "scale=2; $compressed_size / $original_size" | bc)

        # Accumulate sums
        sum_compression_time=$(echo "$sum_compression_time + $compress_time" | bc)
        sum_decompression_time=$(echo "$sum_decompression_time + $decompress_time" | bc)
        sum_compression_ratio=$(echo "$sum_compression_ratio + $compression_ratio" | bc)

        # Clean up after each run
        rm -f "$outfile" "$decompressfile" compress_time.txt decompress_time.txt
    done

    # Calculate averages
    avg_compression_time=$(echo "scale=3; $sum_compression_time / $runs" | bc)
    avg_decompression_time=$(echo "scale=3; $sum_decompression_time / $runs" | bc)
    avg_compression_ratio=$(echo "scale=2; $sum_compression_ratio / $runs" | bc)

    echo "Benchmarking $tool with $infile"
    echo "Average Compression Ratio: $avg_compression_ratio"
    echo "Average Compression Time: $avg_compression_time seconds"
    echo "Average Decompression Time: $avg_decompression_time seconds"
    echo "----------------------------------"
}

# Run benchmarks
for file in "${FILES[@]}"; do
    for tool in "${tools[@]}"; do
        measure "$tool" "$file"
    done
done
