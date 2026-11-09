# Assets

- `macpaint_640x480.bmp`: the 640 x 480 MacPaint window of the sketch's `image.h`, as a 1 bit BMP
  (palette entry 0 black, 1 white). It shows the picture the sketch draws: white drawing area,
  black menu bar and tools. The sketch stores the same picture inverted, with a set bit meaning ink.
- Origin of the picture and of the pen path in `examples/macpaint_hello.rs`: the
  `GDEQ0426T82_MacPaint` sketch, itself a port of upiir's "hello" animation
  (<https://github.com/upiir/seeed_eink_partial_update>). The upstream repository states no licence
  for them that I could find. Check before publishing this repository.
