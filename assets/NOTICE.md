# Assets

- `macpaint_640x480.bmp`: the 640 x 480 MacPaint window of the sketch's `image.h`, as a 1 bit BMP
  (palette entry 0 black, 1 white). It shows the picture the sketch draws: white drawing area,
  black menu bar and tools. The sketch stores the same picture inverted, with a set bit meaning ink.
- Origin of the picture and of the pen path in `examples/macpaint_hello.rs`: the
  `GDEQ0426T82_MacPaint` sketch, itself a port of upiir's "hello" animation
  (<https://github.com/upiir/seeed_eink_partial_update>). The upstream repository states no licence
  for them that I could find. Check before publishing this repository.

- `fonts/Roboto-Black-ASCII.ttf`: Roboto Black 2.001047, Copyright 2015 Google Inc., licensed under
  the Apache License 2.0 (`fonts/LICENSE-Roboto-Apache-2.0.txt`). Source:
  <https://github.com/googlefonts/roboto> (`src/hinted/Roboto-Black.ttf`). Modified: reduced to the
  ASCII characters 0x20 to 0x7E, kerning kept, hinting removed, all name records kept:
  `pyftsubset Roboto-Black.ttf --unicodes=U+0020-007E --layout-features=kern --no-hinting --name-IDs='*'`
  (fonttools). The whole font is 508 KB and `fontdue` runs out of memory loading it on the board,
  because it parses every glyph when a font is opened. Used by `examples/antialias_font.rs`.
