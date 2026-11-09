"""Turn a grayscale animated GIF into a black and white one for the e-paper `gif_player`.

    python tools/dither_gif.py IN.gif OUT.gif [REPEATS]

Needs numpy and pillow. Every frame gets its contrast stretched, then an 8 x 8 ordered dither, so
the picture is pure black and white and a still background gives exactly the same pixels in every
frame (an error diffusion dither would make it shimmer). The whole loop is repeated REPEATS times
(default 4), because `gif_player` plays the file once.
"""
import sys

import numpy as np
from PIL import Image, ImageSequence

BLACK_POINT, WHITE_POINT, GAMMA = 25.0, 175.0, 1.05

BAYER8 = np.array([
    [0, 32, 8, 40, 2, 34, 10, 42], [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38], [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41], [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37], [63, 31, 55, 23, 61, 29, 53, 21]], dtype=np.float32)


def one_bit(frame):
    gray = np.asarray(frame.convert('L'), dtype=np.float32)
    tone = np.clip((gray - BLACK_POINT) / (WHITE_POINT - BLACK_POINT), 0, 1) ** GAMMA
    h, w = tone.shape
    threshold = np.tile((BAYER8 + 0.5) / 64.0, (h // 8 + 1, w // 8 + 1))[:h, :w]
    return (tone > threshold).astype(np.uint8)          # 1 = white


def convert(src, dst, repeats):
    with Image.open(src) as im:
        frames = [one_bit(f) for f in ImageSequence.Iterator(im)]
        durations = [f.info.get('duration', 100) for f in ImageSequence.Iterator(im)]
    palette = [0, 0, 0, 255, 255, 255] + [0] * (254 * 3)
    images = []
    for f in frames * repeats:
        p = Image.fromarray(f, mode='P')
        p.putpalette(palette)
        images.append(p)
    images[0].save(dst, save_all=True, append_images=images[1:], duration=durations * repeats,
                   loop=0, disposal=1, optimize=False)
    print('frames', len(images), 'size', images[0].size)
    return frames


if __name__ == '__main__':
    convert(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 4)
