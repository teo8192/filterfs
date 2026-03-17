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
mount -t fuse.filterfs /source/directory /target/directory -o 'incl=*.m4b,incl=*.pdf'
```

Pass no options to have a RO mirror of all files.

## Options

 - `incl=glob`
    include files matching glob
 - `excl=glob`
    exclude files matching glob
 - `dincl=glob`
    include directories matching glob
 - `dexcl=glob`
    exclude directories matching glob
 - `prune=n`
    how deep to recursively look for empty directories to prune. Default is `0`, i.e. no pruning.
    Beware that pruning may cause performance losses, especially if the underlying directory contains a lot of directories again.

# TODO

There are still some permission trouble, so this needs to be figured out.
