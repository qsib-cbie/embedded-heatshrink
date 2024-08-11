#!/bin/bash

# Ensure hsz is installed
cargo install --path .

# Total number of iterations
total_iterations=100000

# Directory for fuzz data
mkdir -p fuzz

# Function to generate a file with a mix of data types
generate_mixed_file() {
  i=$1
  filename=fuzz/mixed_file_$i.bin
  total_size=$((RANDOM % 1024 * 1024)) # Total size up to 1MB

  {
    while [ $total_size -gt 0 ]; do
      chunk_size=$((RANDOM % 1024 + 1)) # Random chunk size between 1 and 1024 bytes
      case $((RANDOM % 3)) in
        0) # Zero data
          dd if=/dev/zero bs=1 count=$chunk_size 2>/dev/null
          ;;
        1) # Random data
          dd if=/dev/urandom bs=1 count=$chunk_size 2>/dev/null
          ;;
        2) # Text-like data
          LC_ALL=C tr -dc 'a-zA-Z0-9 \n' < /dev/urandom | head -c $chunk_size
          ;;
      esac
      total_size=$((total_size - chunk_size))
    done
  } > "$filename"
}

# Function to process a file
process_file() {
  i=$1
  type=$2
  filename=fuzz/file_$i.bin

  case $type in
    "zero")
      dd if=/dev/zero of="$filename" bs=1k count=$((RANDOM % 10000)) 2> /dev/null
      ;;
    "random")
      dd if=/dev/urandom of="$filename" bs=1k count=$((RANDOM % 10000)) 2> /dev/null
      ;;
    "text")
      LC_ALL=C tr -dc 'a-zA-Z0-9 \n' < /dev/urandom | head -c $((RANDOM % 10000 * 1024)) > "$filename"
      ;;
    "mixed")
      generate_mixed_file "$i"
      ;;

  esac

  hsz < "$filename" > "$filename.hsz"
  hsz -d < "$filename.hsz" > "$filename.decompressed"
  diff "$filename" "$filename.decompressed"

  # Delete files if diff is empty, else exit
  if [ $? -eq 0 ]; then
      rm "$filename" "$filename.hsz" "$filename.decompressed"
  else
      echo "Diff not empty for $filename, exiting"
      exit 1
  fi
}

export -f process_file
export -f generate_mixed_file

# Run zero data tests
echo "Processing zero data files..."
seq $((total_iterations * 1 / 100)) | parallel --bar --halt now,fail=1 -j "$(nproc)" process_file {} zero
if [ $? -ne 0 ]; then
    echo "An error occurred during zero data processing. Exiting."
    exit 1
fi

# Run random data tests
echo "Processing random data files..."
seq $((total_iterations * 24 / 100)) | parallel --bar --halt now,fail=1 -j "$(nproc)" process_file {} random
if [ $? -ne 0 ]; then
    echo "An error occurred during random data processing. Exiting."
    exit 1
fi

# Run text-like data tests
echo "Processing text-like data files..."
seq $((total_iterations * 25 / 100)) | parallel --bar --halt now,fail=1 -j "$(nproc)" process_file {} text
if [ $? -ne 0 ]; then
    echo "An error occurred during text-like data processing. Exiting."
    exit 1
fi

# Run mixed data tests
echo "Processing mixed data files..."
seq $((total_iterations * 40 / 100)) | parallel --bar --halt now,fail=1 -j "$(nproc)" process_file {} mixed
if [ $? -ne 0 ]; then
    echo "An error occurred during mixed data processing. Exiting."
    exit 1
fi