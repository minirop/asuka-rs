![Asuka's face](images/face.png) ![Asuka's name](images/name.png)

A tool to help analyse .cat from the Senran Kagura series.

_Asuka best girl_

# Format

## Header

- 4 bytes: A
- 4 bytes: B
- 4 bytes: C
- 4 bytes: D
- (size - 16) bytes: data

### 1, 1, 0

D is the size of the header. Known values are 0, 8, 16, 32, 64, and 256.

if D is not 0, the following integer is the remaining bytes in that *container* followed by the first 3 bytes of the next header in reverse order.

### 0, B, C

Second header after a "1, 1, 0". B is the number of `children`. C seems to be the file format of the children (see below).

- 4 bytes: 0
- 4 bytes: B
- 4 bytes: C
- 4 bytes: size
- 4 bytes: 0

followed by B file offsets (relative to that header's starting address).
followed by B sizes, the real size of each children (since they are 0x100 aligned).

followed by 0x00 until the end.

Note: Some file have their *size* (if that's really the size) smaller than expected. See `Binary/Text/EN.cat` in `SK:PBS`, has 17 children with a size of 0x40. But the offsets/sizes continue for 0x88 bytes (17 * 8).

#### Values of C

- 0: unknown
- 1: the first child contains the filenames, the other children are tmd0 files.
- 2: each children is a container where the first child contains the filenames, the second child contains the files in one block¹ (only DDS files?).
- 3: same as 1 but with tmo1 files.
- 4: same as 3, don't know the difference.
- 5: depends on the number of children???
    - if 1 or 2, looks like scripts?
    - if 3, names then 2 navmesh
    - if 4, names then starting by [3, 0, 0, 0], also scripts?
- 6: the first child contains the filenames, the second child contains the files in one block¹ (only DDS files? only in `effects/`)
- 7: same as 1 but with unknown files.
- 8: Same as 2, but contains 2 files: mtl.csv and <model>.bin.

¹: a block start with the size of its header, the number of files, a value (probably its size?), then the offsets (relative to the start of the block)

## Example

Just open [sk-pbs.txt](sk-pbs.txt) which contains the result of running the tool on all files from Senran Kagura Peach Beach Splash.
