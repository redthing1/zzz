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
zzz c file.txt -f 7z -p password
zzz c file.txt -f gz -o file.txt.gz

# extract
zzz x archive.zst
zzz x archive.7z -p password -C output/
zzz x file.txt.gz -C output/
zzz x archive.tgz -C output/ --strip-components 1

# list contents
zzz l archive.tgz
zzz l file.txt.xz

# test integrity
zzz t archive.7z
```

Notes:

- raw `.gz`/`.xz` outputs are treated as single-file streams; use `.tgz`/`.txz` (or `.tar.gz`/`.tar.xz`) for tarballs.
- `--strip-components` drops leading path segments during extraction.
- archives strip ownership and xattrs by default; use `--keep-xattrs` to preserve, `--strip-timestamps` for timestamps only, or `--redact` to strip metadata and exclude common secrets.
