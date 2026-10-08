"""Tiny textured software renderer for checking exported OBJ models.

Usage: python render_obj.py model.obj out.png [yaw_deg] [pitch_deg] [size]
"""
import math
import os
import sys

import numpy as np
from PIL import Image


def load(path):
    vs, vts, faces, mats, cur = [], [], [], {}, None
    base = os.path.dirname(path)
    for line in open(path):
        p = line.split()
        if not p:
            continue
        if p[0] == 'v':
            vs.append([float(x) for x in p[1:4]])
        elif p[0] == 'vt':
            vts.append([float(x) for x in p[1:3]])
        elif p[0] == 'usemtl':
            cur = p[1]
        elif p[0] == 'f':
            idx = [tuple(int(x) - 1 if x else -1 for x in (c.split('/') + ['', ''])[:2]) for c in p[1:4]]
            faces.append((idx, cur))
        elif p[0] == 'mtllib':
            mtl = os.path.join(base, p[1])
            name = None
            for ml in open(mtl):
                q = ml.split(maxsplit=1)
                if q and q[0] == 'newmtl':
                    name = q[1].strip()
                elif q and q[0] == 'map_Kd' and os.path.exists(q[1].strip()):
                    mats[name] = np.asarray(Image.open(q[1].strip()).convert('RGBA'), dtype=np.float32)
    return np.array(vs), np.array(vts) if vts else np.zeros((1, 2)), faces, mats


def render(path, out, yaw=35, pitch=25, size=512):
    vs, vts, faces, mats = load(path)
    c = (vs.min(0) + vs.max(0)) / 2
    r = np.linalg.norm(vs.max(0) - vs.min(0)) / 2 or 1
    y, p = math.radians(yaw), math.radians(pitch)
    ry = np.array([[math.cos(y), 0, math.sin(y)], [0, 1, 0], [-math.sin(y), 0, math.cos(y)]])
    rx = np.array([[1, 0, 0], [0, math.cos(p), -math.sin(p)], [0, math.sin(p), math.cos(p)]])
    v = (vs - c) @ ry.T @ rx.T / r
    sx = (v[:, 0] * 0.48 + 0.5) * size
    sy = (0.5 - v[:, 1] * 0.48) * size
    img = np.full((size, size, 3), 40, np.float32)
    zbuf = np.full((size, size), np.inf)
    light = np.array([0.4, 0.8, -0.45])
    for idx, mat in faces:
        a, b, cc = (i for i, _ in idx)
        x = np.array([sx[a], sx[b], sx[cc]]); yy = np.array([sy[a], sy[b], sy[cc]])
        z = np.array([v[a, 2], v[b, 2], v[cc, 2]])
        n = np.cross(v[b] - v[a], v[cc] - v[a]); nl = np.linalg.norm(n)
        shade = 0.45 + 0.55 * abs(n @ light) / (nl or 1)
        x0, x1 = int(max(x.min(), 0)), int(min(x.max() + 1, size))
        y0, y1 = int(max(yy.min(), 0)), int(min(yy.max() + 1, size))
        if x0 >= x1 or y0 >= y1:
            continue
        gx, gy = np.meshgrid(np.arange(x0, x1) + 0.5, np.arange(y0, y1) + 0.5)
        d = (yy[1] - yy[2]) * (x[0] - x[2]) + (x[2] - x[1]) * (yy[0] - yy[2])
        if abs(d) < 1e-9:
            continue
        w0 = ((yy[1] - yy[2]) * (gx - x[2]) + (x[2] - x[1]) * (gy - yy[2])) / d
        w1 = ((yy[2] - yy[0]) * (gx - x[2]) + (x[0] - x[2]) * (gy - yy[2])) / d
        w2 = 1 - w0 - w1
        inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
        zz = w0 * z[0] + w1 * z[1] + w2 * z[2]
        tex = mats.get(mat)
        col = np.full(gx.shape + (3,), 180.0)
        if tex is not None and idx[0][1] >= 0:
            t = np.array([vts[i] for _, i in idx])
            u = w0 * t[0, 0] + w1 * t[1, 0] + w2 * t[2, 0]
            vv = 1 - (w0 * t[0, 1] + w1 * t[1, 1] + w2 * t[2, 1])
            th, tw = tex.shape[:2]
            tx = (np.mod(u, 1) * tw).astype(int).clip(0, tw - 1)
            ty = (np.mod(vv, 1) * th).astype(int).clip(0, th - 1)
            px = tex[ty, tx]
            inside &= px[..., 3] > 8
            col = px[..., :3]
        sub = zbuf[y0:y1, x0:x1]
        m = inside & (zz < sub)
        sub[m] = zz[m]
        img[y0:y1, x0:x1][m] = col[m] * shade
    Image.fromarray(img.clip(0, 255).astype(np.uint8)).save(out)


if __name__ == '__main__':
    a = sys.argv
    render(a[1], a[2], *(float(x) for x in a[3:5]), *(int(x) for x in a[5:6]))
