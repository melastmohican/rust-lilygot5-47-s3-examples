"""Animate the pencil sketch of Jimothy as a diagonal gait walk cycle, as a 1 bit GIF.

    python tools/jimothy_walk.py SKETCH.jpg OUT.gif

Needs numpy, pillow and scipy. The sketch is a pencil drawing of a raccoon in mid stride, 4:3, the
leg and head positions below are in its 723 x 540 pixels. The GIF is 432 x 322, 16 frames per step,
2 steps. Frame by frame the regions of the picture are moved with smooth displacement fields: the
raised front paw and the forward hind foot swing together (one diagonal), the planted feet swing
the other way, the body bobs twice per step, the head nods and the tail sways. Then each frame is
cut to black and white, so the GIF suits the e-paper `gif_player` example.
"""
import sys

import numpy as np
from PIL import Image
from scipy.ndimage import map_coordinates

SRC = sys.argv[1] if len(sys.argv) > 1 else 'jimothy_sketch.jpg'
OUT_W, OUT_H = 432, 322
N = 16                     # frames per walk cycle
SX = OUT_W / 723.0         # source -> output scale

img = Image.open(SRC).convert('L').resize((OUT_W, OUT_H), Image.LANCZOS)
base = np.asarray(img, dtype=np.float32)
yy, xx = np.mgrid[0:OUT_H, 0:OUT_W].astype(np.float32)

def blob(cx, cy, sx, sy):
    """Gaussian weight around a source-image point, cut to zero in the far field."""
    cx, cy, sx, sy = cx * SX, cy * SX, sx * SX, sy * SX
    w = np.exp(-(((xx - cx) ** 2) / (2 * sx ** 2) + ((yy - cy) ** 2) / (2 * sy ** 2)))
    return np.clip((w - 0.03) / 0.97, 0, 1)

# Regions, in source pixels (723 x 540).
W_RAISED = blob(448, 360, 34, 55)     # raised front paw and forearm
W_HIND2 = blob(305, 395, 42, 30)      # hind foot that is stepping forward
W_FRONT = blob(420, 455, 36, 40)      # planted front foot
W_HIND = blob(150, 480, 45, 45)       # planted hind foot
W_HEAD = blob(490, 205, 55, 60)
W_TAIL = blob(150, 185, 55, 60)
W_BODY = blob(300, 230, 190, 140)

def displacement(t):
    """(dx, dy) in output pixels at cycle position t in 0..1."""
    phi_a = 1.5 * np.pi + 2 * np.pi * t       # pair A (raised paw + forward hind foot): mid swing at t = 0
    phi_b = phi_a + np.pi                       # pair B (planted feet): mid stance at t = 0
    reach = 44.0 * SX                           # how far a foot travels from the rest pose
    lift = 40.0 * SX                            # how high a foot lifts in the swing
    def foot(phi, rest_lifted):
        dx = reach * np.cos(phi)
        h = lift * max(0.0, -np.sin(phi))
        rest = lift if rest_lifted else 0.0
        return dx, -(h - rest)
    ax, ay = foot(phi_a, True)
    bx, by = foot(phi_b, False)
    dx = ax * (W_RAISED + W_HIND2) + bx * (W_FRONT + W_HIND)
    dy = ay * (W_RAISED + W_HIND2) + by * (W_FRONT + W_HIND)
    # Body: bounces twice per cycle, rolls with the stride.
    bob = 6.0 * SX * np.sin(2 * (phi_a - 1.5 * np.pi) + 0.4)
    dy = dy + bob * W_BODY
    dx = dx + 5.0 * SX * np.sin(phi_a - 1.5 * np.pi + 0.3) * W_BODY
    # Head: nods against the bounce and leads the stride.
    dy = dy + 7.0 * SX * np.sin(2 * (phi_a - 1.5 * np.pi) - 0.9) * W_HEAD
    dx = dx + 7.0 * SX * np.cos(phi_a - 1.5 * np.pi) * W_HEAD
    # Tail: sways one way per step.
    dx = dx + 11.0 * SX * np.sin(2 * (phi_a - 1.5 * np.pi) + 1.2) * W_TAIL
    dy = dy + 8.0 * SX * np.sin(phi_a - 1.5 * np.pi + 0.8) * W_TAIL
    return dx, dy

def warp(t):
    dx, dy = displacement(t)
    # Backward mapping: the pixel that lands at (x, y) came from (x - dx, y - dy).
    return map_coordinates(base, [yy - dy, xx - dx], order=1, mode='nearest')

BAYER8 = np.array([
    [0, 32, 8, 40, 2, 34, 10, 42], [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38], [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41], [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37], [63, 31, 55, 23, 61, 29, 53, 21]], dtype=np.float32)
THRESH = (BAYER8 + 0.5) / 64.0

def to_one_bit(gray, mode):
    tone = np.clip((gray - 40.0) / (205.0 - 40.0), 0, 1)
    if mode == 'threshold':
        return (tone > 0.62).astype(np.uint8)
    tone = tone ** 1.25
    th = np.tile(THRESH, (OUT_H // 8 + 1, OUT_W // 8 + 1))[:OUT_H, :OUT_W]
    return (tone > th).astype(np.uint8)      # 1 = white

def frames(mode):
    return [to_one_bit(warp(k / N), mode) for k in range(N)]


CYCLES = 2


def write_gif(path):
    from PIL import Image

    frames_all = frames('threshold') * CYCLES
    palette = [0, 0, 0, 255, 255, 255] + [0] * (254 * 3)
    images = []
    for f in frames_all:
        im = Image.fromarray(f.astype(np.uint8), mode='P')
        im.putpalette(palette)
        images.append(im)
    images[0].save(path, save_all=True, append_images=images[1:], duration=120, loop=0,
                   disposal=1, optimize=False)
    print('frames', len(images), 'size', images[0].size)


if __name__ == '__main__':
    write_gif(sys.argv[2] if len(sys.argv) > 2 else 'jimothy_walk.gif')
