# Third-party material

## Palette data

Names and HEX values for the 24-, 144-, and 221-color MARD tables come from
the MARD section of <https://beiyapd.com/zh-CN/bead-tools#mard>. The smaller
tables are subsets: a shared code uses the same name and RGB in every table.

This records the source used for the color data. It is not a claim that the
upstream chart, names, or artwork are covered by this repository's MIT license.

## Pattern sheet font and mark

`assets/SheetFont.ttf` is a SimHei subset used only to draw pattern-sheet
labels: Latin letters, digits, and the Chinese phrases on the chart. The file
comes from the local pixel-downsample pattern sheet. SimHei is not covered by
this repository's MIT license.

`assets/xiaohongshu-logo.png` is the Xiaohongshu mark shown beside 豆图工坊 on
pattern sheets, copied from the same local project. It is a third-party
trademark and is not covered by this repository's MIT license.

## Dependencies and images

Rust dependencies retain their own licenses; exact versions are in Cargo.lock.
Selected existing output images are included in docs/images at the maintainer's
request for README galleries. Their original paths and export dimensions are
listed in docs/images/README.md. This does not grant rights to any underlying
third-party reference material. Local reference photographs, screenshots, and
the rest of the output directory remain excluded.
