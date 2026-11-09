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

- `mocha800x480.jpg`: the 800 x 480 JPEG of the `show_jpeg` sketch (the same picture as the
  `mocha800x480.png` of the M5Stack PaperDIY `microSD` sketch). A photograph of Mocha, an Australian
  Labradoodle, taken by the author of this repository, who owns the copyright.

- `jimothy_painting.jpg`, `jimothy_sketch.jpg`, `jimothy_manga.jpg`: three AI-generated pictures of a
  raccoon, made by the author of this repository, who owns whatever rights there are in them. Each
  is the original (2400 x 1792) scaled to 723 x 540 and saved as JPEG at quality 85. Used by
  `examples/draw_images.rs`.

- `muybridge_horse_walking.gif`: Eadweard Muybridge's photographs of a horse walking (Animal
  Locomotion, 1880s), public domain. Animated by Wikimedia Commons user Waugsberg as
  `Muybridge horse walking animated.gif`
  (<https://commons.wikimedia.org/wiki/File:Muybridge_horse_walking_animated.gif>, licence: public
  domain), 342 x 246, 9 frames. Modified here: contrast stretched, converted to pure black and white
  with an 8 x 8 ordered dither, and the 9 frame stride repeated 4 times, with
  `python tools/dither_gif.py Muybridge_horse_walking_animated.gif assets/muybridge_horse_walking.gif 4`.
  Used by `examples/gif_player.rs`. It replaces the pixel art city GIF of the Arduino sketch, whose
  author and licence were unknown.

- `sd-card/MOCHA.PNG`: the 800 x 480 PNG of Mocha that the `micro_sd` example reads from the microSD
  card (the sketch's `mocha800x480.png`). A photograph of Mocha, an Australian Labradoodle, taken by
  the author of this repository, who owns the copyright. The same picture as `mocha800x480.jpg`.
