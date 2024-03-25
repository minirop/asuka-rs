![Asuka's face](images/face.png) ![Asuka's name](images/name.png)

A tool to help analyse .cat from the Senran Kagura series.

_Asuka best girl_

# Usage

## Print the structure of the files in a directory

```console
$ asuka <directory> [-m 16]
```

By default, only show the first 8 integers, but that can be increased with the `-m` parameter.

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
- 4 bytes: 1
- 4 bytes: 0
- 4 bytes: header size
- 4 bytes: content size
- (size - 20) bytes: data
- 4 bytes: 0 // this is the `byte 0` for files offsets
- 4 bytes: number of children
- 4 bytes: type
- 4 bytes: aligment of children
- 4 bytes: 0
- 4 * children bytes: start of children (relative to `byte 0`, hence the first file having an offset of `header size` and not 0)
- 4 * children bytes: size of children
- 0x00 until aligment

Note: The end of `data` (most of the time just after `content size`) are the next 3 bytes but backwards (`type`, `number of children`, `0`)

#### Values of "type"

- 0: list of "containers"
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

# Files that don't work

All filenames are from `Peach Beach Splash`.

## corrupted(?) files

For instance, `Motion/Zako/zk**_act_vis.cat`. The 2nd header says it is 256 bytes, but the entire file is 284 bytes. (or maybe I don't parse them correctly)

## Empty(?) files

For instance, `Ui/Adv_Illust/adv_ilst_ev0**.cat` starting at `34` (+ `00`). Their header have a size of 0, but the children are 0x100 aligned and filled with `0x00`.

headers are as follow:
- at `0x0000`: `[1, 1, 0, 256, 768, 2, 1, 0, ...]`
- at `0x0100`: `[0, 1, 2, 256, 0, 256, 288, 0, ...]`
    - This says its child is from `0x200` to `0x320`.
    - But there are also headers at `0x200` and `0x300`.
- at `0x0200`: `[1, 1, 0, 0, 0, 0, 0, 0, ...]`
- at `0x0300`: `[0, 2, 0, 256, 0, 256, 256, 0, ...]`
    - This says it has 2 children both starting at 0x400 with a size of 0 bytes
