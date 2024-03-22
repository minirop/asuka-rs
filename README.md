![Asuka's face](images/face.png) ![Asuka's name](images/name.png)

A tool to help analyse .cat from the Senran Kagura series.

_Asuka best girl_

# Format

All files start with some headers (generally 4, but not always) than seems to have that format:

- 4 bytes: A
- 4 bytes: B
- 4 bytes: C
- 4 bytes: header's size
- size - 16 bytes: data

## Textures (e.g. Ui/Adv_Illust/)

| header # | A | B | C | size |
|----------|---|---|---|------|
| #1 | 1 | 1 | 0 | 256 |
| #2 | 0 | 1 | 2 | 256 |
| #3 | 1 | 1 | 0 | 256 |
| #4 | 0 | 2 | 0 | 256 |

## Binary/Adv/

| header # | A | B | C | size |
|----------|---|---|---|------|
| #1 | 1 | 1 | 0 | 32 |
| #2 | 0 | X | 0 | 16 |

X seems to be the number of `chunks`. Then there is a list of increasing values (offsets?) and finally the `chunks`.

`chunks` are lists filenames separated by 22 bytes with that format:
filename 0x00
0x20 0x20
4 bytes that are unknown
15 0x00 bytes

Between each chunk (or before?), there are 0x100 bytes of unknown meaning.
