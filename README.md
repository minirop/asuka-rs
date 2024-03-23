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

Second header after a "1, 1, 0". B is the number of `children`. C is unknown.

- 4 bytes: 0
- 4 bytes: B
- 4 bytes: C
- 4 bytes: size
- 4 bytes: 0

followed by B file offsets (relative to that header's starting address).
followed by B sizes, the real size of each children (since they are 0x100 aligned).

followed by 0x00 until the end.

Note: Some file have their *size* (if that's really the size) smaller than expected. See `Binary/Text/EN.cat` in `SK:PBS`, has 17 children with a size of 0x40. But the offsets continues for 0x88 bytes (17 * 8).
