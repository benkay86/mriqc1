# mriqc1

Run [mriqc](https://mriqc.org/) one participant at a time, in parallel.  Specify the number of parallel instances to throttle system resource usage.

* [Installation](#installation)
* [Usage](#usage)

## Installation

You will need to [install mriqc](https://mriqc.readthedocs.io/en/latest/install.html) and its dependencies separately.

This application uses the [Rust](https://www.rust-lang.org/) programming language for fast, efficient, and reliable concurrency.  If your lab provides a pre-built mriqc1 binary then you do *not* need to install any Rust-related dependencies.  Simply run the binary.

### Installing Rust

To build mriqc1 you will need to [install Rust](https://www.rust-lang.org/tools/install) and its build tool cargo if you have not done so already.  Conventionally, these tools are installed in your home directory rather than system-wide.  First-time installation is typically as simple as:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

And you can update to the latest version with:

```
rustup update stable
```

### Using Git

This application uses [git](https://git-scm.com) for version management.  Your Linux distribution most likely provides pre-built packages for git.  To download this project and its submodules for the first time:

```
git clone https://github.com/benkay86/mriqc1
cd mriqc1
git submodule update --init --recursive
```

To pull down new changes, i.e. update your source tree:

```
git pull
git submodule update --init --recursive
```

### Building with Cargo

Rust's [cargo](https://doc.rust-lang.org/cargo/) build tool is very easy to use.

```
cargo build --release
cargo run --release -- [arguments to mriqc1]
```

Or you can manually invoke the binary at `target/release/mriqc1`.

## Usage

### Background

[mriqc](https://mriqc.org) is a tool for generating quality-control metrics on MRI data.  It uses [nipype](https://nipy.org/packages/nipype/index.html) under-the-hood to process data from multiple MRI study participants (i.e. subjects)  in parallel.  Sometimes when processing a very large number of subjects mriqc will exhaust system resources and crash.  (The `--nprocs` option does not appear to limit RAM usage.)

mriqc1 is a harness for mriqc which processes just one subject at a time, but can be configured to run multiple instances of mriqc in parallel.  By specifying the the number of parallel mriqc instances you gain finer control over system resource usage.

### Running mriqc

Please refer to the [documentation for mriqc](https://mriqc.readthedocs.io/en/latest/running.html).  As an example, suppose you have directory of participants' data organized according to the [BIDS specification](https://bids.neuroimaging.io/specification) at `/bids`.  You want to run mriqc on the T1-weighted (T1w) weighted scans for participants `bob` and `susan` and store the output in the directory `/out`:

```
mriqc /bids /out participant --participant-label bob susan -m T1w
```

Most of the options in the above command are self-explanatory.  The positional argument `participant` tells mriqc to run a participant-level analysis (as opposed to a group-level meta-analysis).  One or more participant identifiers is specified after `--participant-label`.  The `-m` option tells mriqc to just process data from the T1w modality.

### Running mriqc1

mriqc1 accepts most of the same options as mriqc.  To run the preceding example with mriqc1:

```
mriqc1 --bids-dir /bids --out-dir /out --participant-label bob susan -- -m T1w
```

The `--bids-dir` and `--out-dir` options are required to explicitly specify the BIDS and output directories, respectively.  Otherwise the arguments are very similar to mriqc.  You can pass through any extra arguments not supported by mriqc1 to mriqc by placing them after the `--`.  In this case, mriqc1 does not understand the `-m T1w` argument so we pass it through to mriqc.

Run `mriqc --help` to see a full list of supported arguments.  The `-n` option controls how many instances of mriqc to run in parallel and defaults to `-n 1`.

### Advanced Usage

Combine mriqc1 with features of the [bash shell](https://en.wikipedia.org/wiki/Bash_%28Unix_shell%29) to achieve more complex processing objectives.  In the following example we process T1-weighted data with `-m T1w` for 3 participants at a time with `-n 3` from a list of participants in a newline-delimited file with `$(tr ...)`.  We manually specify the temporary/working directory with `--work-dir`, pipe warnings to a log file for later inspection with `2>log.txt`, and opt-out of mriqc's telemetry with `--no-sub`.

```
mriqc1 -n 3 --bids-dir /bids --out-dir /out --work-dir /tmp \
--participant-label $(tr '\n' ' ' < participants.txt) -- \
-m T1w --no-sub 2> log.txt
```
```
Running mriqc, this could take a long time. press Ctrl+C to cancel...
(25/100 participants): 5h [====================>                ] 15h
Running mriqc on participant NDARINV11111111 .
Running mriqc on participant NDARINV22222222 ..
Running mriqc on participant NDARINV33333333 ...
```

### Help

Here is the output of `mriqc --help` for reference.  Feel free to contact the main author [Benjamin Kay](mailto:benjamin@benkay.net) for assistance.

```
USAGE:
    mriqc1 [FLAGS] [OPTIONS] --bids-dir <bids-dir> --out-dir <out-dir> --participant-label <participant-labels>... [--] [extra-args]...

FLAGS:
    -h, --help       Prints help information
    -q, --quiet      Be quite, don't show progress bar or warnings
        --resume     Skip participants for whom any data is already present in the output directory
    -V, --version    Prints version information
        --werror     Convert warnings about failure to process a participant to errors and exit on the first error.  This does not apply to timeout warnings

OPTIONS:
        --bids-dir <bids-dir>                          BIDS directory containing data
        --timeout <minutes>                            Cancel a participant's mriqc process if it runs longer than this many minutes
        --mriqc <mriqc>                                Location of mriqc binary [env: MRIQC=]  [default: mriqc]
        --out-dir <out-dir>                            Directory for output files
    -n <parallel>                                      Number of participants to run in parallel [default: 1]
        --participant-label <participant-labels>...    Participant label(s)
    -w, --work-dir <work-dir>                          Working directory for temporary files, defaults to system tempdir

ARGS:
    <extra-args>...    Extra arguments to pass through to mriqc
```

## License

This project is licensed under either of

* Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.
