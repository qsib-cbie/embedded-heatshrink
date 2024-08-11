"""
Load the test output with lines like:
'''
RoundtripConfig { window_sz2: 6, lookahead_sz2: 4, in_read_sz: 4096, out_read_sz: 8, out_buffer_sz: 512, file_name: "heatshrink_encoder.rs", compressed_size: 14829, compression_ratio: 1.7059141, compression_time_us: 1100 }
RoundtripConfig { window_sz2: 6, lookahead_sz2: 4, in_read_sz: 4096, out_read_sz: 8, out_buffer_sz: 512, file_name: "random-data.bin", compressed_size: 73671, compression_ratio: 0.8895766, compression_time_us: 4102 }
RoundtripConfig { window_sz2: 7, lookahead_sz2: 4, in_read_sz: 8, out_read_sz: 1, out_buffer_sz: 1, file_name: "tsz-compressed-data.bin", compressed_size: 1361374, compression_ratio: 1.427546, compression_time_us: 92271 }
RoundtripConfig { window_sz2: 7, lookahead_sz2: 4, in_read_sz: 8, out_read_sz: 1, out_buffer_sz: 64, file_name: "text.txt", compressed_size: 8, compression_ratio: 0.875, compression_time_us: 0 }
RoundtripConfig { window_sz2: 7, lookahead_sz2: 4, in_read_sz: 8, out_read_sz: 1, out_buffer_sz: 64, file_name: "heatshrink_encoder.rs", compressed_size: 12477, compression_ratio: 2.0274906, compression_time_us: 1057 }
RoundtripConfig { window_sz2: 7, lookahead_sz2: 4, in_read_sz: 8, out_read_sz: 1, out_buffer_sz: 64, file_name: "random-data.bin", compressed_size: 73632, compression_ratio: 0.8900478, compression_time_us: 4058 }
RoundtripConfig { window_sz2: 4, lookahead_sz2: 3, in_read_sz: 1, out_read_sz: 1, out_buffer_sz: 512, file_name: "tsz-compressed-data.bin", compressed_size: 1395170, compression_ratio: 1.3929657, compression_time_us: 123276 }
'''

For tsz-compressed-data.bin, we will plot a bar chart of the average compression ratio for each window_sz2 and lookahead_sz2.
The bar chart will plot all of one window_sz2 together, with each lookahead_sz2 as a different color. This will allow us to decide which window_sz2
is the best in general and fine-tune an appropriate lookahead_sz2.
"""

import matplotlib.pyplot as plt
import numpy as np


def main():
    file_name = "sanity_param_sweep.txt"
    configs = {}

    # Read the results from each file
    with open(file_name) as f:
        lines = f.readlines()
        for line in lines:
            if "RoundtripConfig {" in line:
                line = line.split("RoundtripConfig { ")[1].split(" }")[0]
                line = line.split(", ")
                config = {}
                for item in line:
                    key, value = item.split(": ")
                    if key == "window_sz2" or key == "lookahead_sz2":
                        config[key] = int(value)
                    elif key == "file_name":
                        config[key] = value[1:-1]
                    else:
                        config[key] = float(value)

                if config["file_name"] not in configs:
                    configs[config["file_name"]] = []
                configs[config["file_name"]].append(config)

    # Aggregate the average compression ratio for each window_sz2 and lookahead_sz2
    compression_ratios = {}
    compression_times = {}
    for file_name, config_list in configs.items():
        if file_name != "tsz-compressed-data.bin":
            continue
        for config in config_list:
            key = (config["window_sz2"], config["lookahead_sz2"])
            if key not in compression_ratios:
                compression_ratios[key] = []
            compression_ratios[key].append(config["compression_ratio"])
            if key not in compression_times:
                compression_times[key] = []
            compression_times[key].append(config["compression_time_us"])

    # Compute the averages for each window_sz2 and lookahead_sz2 pair
    avg_ratios = {}
    for key, ratios in compression_ratios.items():
        avg_ratios[key] = sum(ratios) / len(ratios)
    avg_times = {}
    for key, times in compression_times.items():
        avg_times[key] = sum(times) / len(times)

    print(avg_ratios)

    # Plot a bar chart of the average compression ratios with each window_sz2 as a different color
    num_lookaheads = len(set([key[1] for key in avg_ratios.keys()]))
    sorted_keys = sorted(avg_ratios.keys())
    window_sz2s = sorted(set([key[0] for key in avg_ratios.keys()]))
    lookahead_sz2s = sorted(set([key[1] for key in avg_ratios.keys()]))
    bar_width = 0.075

    # Put the comrpession ratios on the first row and the compression times on the second row
    fig, axs = plt.subplots(2, 1)
    ax = axs[0]

    # Use the same color for each lookahead_sz2
    colors = plt.get_cmap("tab20", len(lookahead_sz2s))
    for i, window_sz2 in enumerate(window_sz2s):
        # Space out the window_sz2s to tightly group the lookahead_sz2s together
        for j, lookahead_sz2 in enumerate(lookahead_sz2s):
            # Group each of the lookahead_sz2s together one after the other
            if (window_sz2, lookahead_sz2) in avg_ratios:
                ax.bar(
                    i + j * bar_width,
                    avg_ratios[(window_sz2, lookahead_sz2)],
                    bar_width,
                    color=colors(lookahead_sz2 - lookahead_sz2s[0]),
                )

    ax.set_xlabel("Window Size (2^x)")
    ax.set_ylabel("Average Compression Ratio")
    ax.set_title(
        "Average Compression Ratio for Different Window Sizes and Lookahead Sizes"
    )
    ax.set_xticks(np.arange(len(window_sz2s)))
    ax.set_xticklabels(window_sz2s)

    # Plot a bar chart of the average compression times with each window_sz2 as a different color
    ax = axs[1]
    for i, window_sz2 in enumerate(window_sz2s):
        for j, lookahead_sz2 in enumerate(lookahead_sz2s):
            if (window_sz2, lookahead_sz2) in avg_times:
                ax.bar(
                    i + j * bar_width,
                    avg_times[(window_sz2, lookahead_sz2)],
                    bar_width,
                    color=colors(lookahead_sz2 - lookahead_sz2s[0]),
                )

    ax.set_xlabel("Window Size (2^x)")
    ax.set_ylabel("Average Compression Time (us)")
    ax.set_title(
        "Average Compression Time for Different Window Sizes and Lookahead Sizes"
    )
    ax.set_xticks(np.arange(len(window_sz2s)))
    ax.set_xticklabels(window_sz2s)

    plt.show()


if __name__ == "__main__":
    main()
