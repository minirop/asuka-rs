![Asuka's face](images/face.png) ![Asuka's name](images/name.png)

A tool to help analyse .cat from the Senran Kagura series.

_Asuka best girl_

# Usage

## Print the structure of the files in a directory

```console
$ asuka <directory>
```

## Extract all files from a directory

```console
$ asuka <directory> -e <output_directory>
```

This will retain the full directory structure.

## Pack a directory into a .cat file

```console
$ asuka <directory> -p <filename.cat>
```

The directory must contains `metadata.json`.

# Format

## Header

- 4 bytes: 1
- 4 bytes: 1 (it has a value of 2 for cameras in `Camera/Action`)
- 4 bytes: 0
- 4 bytes: header size
- 4 bytes: content size
- (size - 20) bytes: data
- 4 bytes: 0 // this is `byte 0`
- 4 bytes: number of children
- 4 bytes: type
- 4 bytes: aligment of children
- 4 bytes: 0
- 4 * children bytes: offsets of children (relative to `byte 0`, hence the first offset not being `0`)
- 4 * children bytes: sizes of children
- 0x00 until aligment

Note: The end of `data` (most of the time just after `content size`) are the next 3 bytes but backwards (`type`, `number of children`, `0`)

#### Values of "type"

- 0: list of "containers"(?)
- 1: the first child contains the filenames, the other children are tmd0 files.
- 2: each children is a container where the first child contains the filenames, the second child contains the files in one block¹ (only DDS files?).
- 3: same as 1 but with tmo1 files. (except for cameras in `1, 2, 0`)
- 4: same as 3, don't know the difference.
- 5: depends on the number of children???
    - if 1 or 2, looks like scripts?
    - if 3, names then 2 navmesh
    - if 4, names then starting by [3, 0, 0, 0], also scripts?
- 6: the first child contains the filenames, the second child contains the files in one block¹ (only DDS files? only in `effects/`)
- 7: same as 1 but with unknown files.
- 8: Same as 2, but contains 2 files: mtl.csv and <model>.bin.

¹: a block start with the size of its header, the number of files, its size, then the offsets (relative to the start of the block)

## Example

Just open [sk-pbs.txt](sk-pbs.txt) which contains the result of running the tool on all files from Senran Kagura Peach Beach Splash. (not necessarily up to date)

## DXT1 1-bit alpha

Some transparent files (like swim_blend00_EE in some swimsuits), are using DXT1 with 1-bit alpha. The crate I use don't support this so you'll need to edit metadata.json to set them to DXT5 otherwise you'll get pitch black clothes.