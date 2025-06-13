# zzz

a fast compression multitool

## features

- supports zst, tgz, txz, zip, 7z formats
- optional encryption for zst and 7z
- smart file filtering with excludes
- streaming compression with threading

## build

```sh
cargo build --release
```

## install

```sh
cargo install --path .
```

## usage

```sh
# compress
zzz c input/ -o archive.zst
zzz c file.txt -f zip -p password

# extract  
zzz x archive.zst
zzz x archive.zip -p password -C output/

# list contents
zzz l archive.tgz

# test integrity
zzz t archive.7z
```