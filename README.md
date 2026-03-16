# FilterFS

A small FUSE-filesystem to mirror another directory (RO), but filter it slightly based on file endings.

# Install

Compile

```sh
git clone git@github.com:teo8192/filterfs.git
cd filterfs
cargo build --release
```

Install

```sh
sudo cp target/release/filterfs
```

# Usage

To mount a RO mirror that only shows `m4b` and `pdf` files:

```sh
mount -t fuse.filterfs /source/directory /target/directory -o include=m4b,include=pdf
```

Pass no options to have a RO mirror of all files.

There are still some permission trouble, so this needs to be figured out.
