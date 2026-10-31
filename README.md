# png-compare

A small command-line tool that answers two questions about PNG pictures:

- **`diff`**: how different are these two pictures?
- **`cast`**: does this picture lean towards a colour, and in which tones?

I wrote it to check photo renders, for example whether a CPU and a GPU render came out the same, or whether a change
leaves purple in the deep shadows. It works on any PNG.

## Build

```sh
cargo build --release
```

The program is then `target/release/png-compare` (`.exe` on Windows). It needs Rust 1.88 or newer and has been tested
on Windows only, with Rust 1.99.

## Compare two pictures

```text
$ png-compare diff before.png after.png
size             1600x1067
identical        no
mean difference  43.0691 levels
largest          74 levels, at pixel (527, 153)
off by over 1    5060494 of 5121600 values
off by over 3    5040793 of 5121600 values
```

Each pixel has three values (red, green, blue), and a level is one step of the 0–255 scale.

| Line | Meaning |
|---|---|
| `mean difference` | The average change per value, so small scattered changes and a few large ones can look alike |
| `largest` | The single biggest change, and where to look for it (x, y from the top left) |
| `off by over 1` | How many values changed by more than rounding could explain |
| `off by over 3` | How many values changed enough to matter; a difference of 1 or 2 levels is hard to see |

The two pictures must be the same size.

## Look for a colour cast

```text
$ png-compare cast after.png
size 1600x1067
band      share     mean R  mean G  mean B  magenta
 32-64     0.32 %    60.28   58.62   54.04    -1.47
 64-128   10.62 %   104.35  106.48   93.80    -7.41
128-256   89.07 %   199.62  198.76  189.47    -4.21
```

Pixels are sorted by brightness into bands: 0–8, 8–16, 16–32, 32–64, 64–128 and 128–255. The bands are finer in the
shadows, where a cast is easiest to see. Empty bands are left out.

| Column | Meaning |
|---|---|
| `share` | The part of the picture in that band |
| `mean R/G/B` | The average of each channel; equal values mean the band is neutral on average |
| `magenta` | `(R + B) / 2 - G`: above zero leans magenta or purple, below zero leans green |

`magenta` only measures green against red and blue together. A yellow or blue cast shows up in the `mean` columns
instead, as in the example above, where blue is lower than red and green.

These are averages. A band can average out neutral while some of its pixels are strongly coloured.

## What happens to a picture first

Every picture is reduced to 8-bit RGB before anything is measured:

- 16-bit samples keep their high byte, so differences smaller than one 8-bit level don't count.
- Grey and palette pictures become RGB.
- Alpha is ignored.
- Nothing is colour managed. Embedded profiles and gamma are ignored, and brightness is computed from the stored values
  with Rec. 709 weights. Compare pictures that are in the same colour space.

A picture may take up to 4 GiB once decoded.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done (for `diff`: the pictures are identical) |
| 1 | `diff`: the pictures differ |
| 2 | Trouble: wrong command line, unreadable file or different sizes |

## Licence

[MIT](LICENSE).
