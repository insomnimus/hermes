# Hermes
Hermes is a command line tool that splits cuesheet + image files into separate tracks with metadata, similar to [CueTools](https://github.com/gchudov/cuetools.net).

## Runtime Dependencies
Only the `ffmpeg` tool is required.

## Installation
Grab a release archive from the [releases page](https://github.com/insomnimus/hermes/releases), or build it from source.

## Build The Code
### Build Requirements
- The Rust toolchain version 1.79.0 or newer

### Build Steps
Simply run:
```shell
cargo build --release
```

The executable will be at `target/release/hermes` (with a `.exe` extension on Windows); you can copy it into another directory.

## Usage
Briefly you provide a path to a `.cue` file, or a directory containing one or more `.cue` files, optionally specify an output directory and a file naming scheme:

```shell
# Split `foo.cue` and save files in `out`
hermes foo.cue -o out

# Use an encoder preset: libmp3lame on high quality
hermes foo.cue -o out -p libmp3lame-high

# Provide custom encoding options to ffmpeg
# Note that the `--ext` option is required in this case
hermes foo.cue -o out --ext ogg -- -acodec libopus -f oga -cutoff 18000 -b 256k

# Use a different file naming scheme and split every .cue file in the current directory recursively
hermes . -o ~/music --template "<artist>/<year> - <album>/<no>. <title>.<ext>"

# To view template help
hermes --template-help
# And to list available presets and how they call ffmpeg
hermes --list-presets
```
