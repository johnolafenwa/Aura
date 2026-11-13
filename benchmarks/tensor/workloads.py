"""The tensor workloads in PyTorch.

    python3 workloads.py cpu
    python3 workloads.py cuda
    python3 workloads.py mps
    python3 workloads.py cuda small

Each row prints `<name> <size> <milliseconds> <result> <first pass
milliseconds>`, the same rows as `workloads.au`. The random numbers
differ from Aura's, so a result is close to Aura's, not equal to it.
"""

import math
import sys
import time

import torch
import torch.nn.functional as F
from torch import nn


def now():
    return time.perf_counter() * 1000.0


def mlp_train(rows, steps, device):
    batch = 256
    x = torch.randn(rows, 784, device=device)
    labels = (x @ torch.randn(784, 10, device=device)).argmax(-1)
    model = nn.Sequential(nn.Linear(784, 512), nn.ReLU(), nn.Linear(512, 10)).to(device)
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.001, weight_decay=0.01)
    start = now()
    first = 0.0
    last = 0.0
    for step in range(steps + 1):
        at = (step * batch) % (rows - batch + 1)
        loss = F.cross_entropy(model(x[at : at + batch]), labels[at : at + batch])
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        if step == 0:
            last = loss.item()
            first = now() - start
            start = now()
        if step == steps:
            last = loss.item()
    print(f"mlp_train {steps} {now() - start:.0f} {last} {first:.0f}")


def mlp_infer(rows, reps, device):
    x = torch.randn(rows, 784, device=device)
    model = nn.Sequential(
        nn.Linear(784, 2048), nn.ReLU(), nn.Linear(2048, 2048), nn.ReLU(), nn.Linear(2048, 10)
    ).to(device)
    start = now()
    first = 0.0
    total = 0
    with torch.no_grad():
        for rep in range(reps + 1):
            total = model(x).argmax(-1).sum().item()
            if rep == 0:
                first = now() - start
                start = now()
    print(f"mlp_infer {rows} {now() - start:.0f} {total} {first:.0f}")


def kmeans(rows, rounds, device):
    width, count = 64, 256
    points = torch.randn(rows, width, device=device)
    squares = (points * points).sum(1, keepdim=True)
    ones = torch.ones(rows, 1, device=device)
    centres = points[:count]
    start = now()
    first = 0.0
    moved = 0.0
    for round in range(rounds + 1):
        distances = squares - points @ centres.T * 2.0 + (centres * centres).sum(1, keepdim=True).T
        nearest = distances.argmin(-1)
        sums = torch.zeros(count, width, device=device).index_add_(0, nearest, points)
        sizes = torch.zeros(count, 1, device=device).index_add_(0, nearest, ones).clamp(min=1.0)
        following = sums / sizes
        moved = (following - centres).abs().sum().item()
        centres = following
        if round == 0:
            first = now() - start
            start = now()
    print(f"kmeans {rows} {now() - start:.0f} {moved} {first:.0f}")


def conjugate_gradient(side, rounds, device):
    m = torch.randn(side, side, device=device)
    a = m @ m.T / side + torch.eye(side, device=device)
    b = torch.randn(side, 1, device=device)
    x = torch.zeros(side, 1, device=device)
    r = b.clone()
    p = b.clone()
    rr = (r * r).sum()
    start = now()
    first = 0.0
    for round in range(rounds + 1):
        ap = a @ p
        alpha = rr / (p * ap).sum()
        x = x + p * alpha
        r = r - ap * alpha
        following = (r * r).sum()
        p = r + p * (following / rr)
        rr = following
        if round == 0:
            sync(device)
            first = now() - start
            start = now()
    left = (a @ x - b).abs().max().item()
    print(f"conjugate_gradient {side} {now() - start:.0f} {left} {first:.0f}")


class Block(nn.Module):
    def __init__(self, width, heads=4):
        super().__init__()
        self.heads = heads
        self.norm1 = nn.LayerNorm(width)
        self.query = nn.Linear(width, width)
        self.key = nn.Linear(width, width)
        self.value = nn.Linear(width, width)
        self.output = nn.Linear(width, width)
        self.norm2 = nn.LayerNorm(width)
        self.wide = nn.Linear(width, width * 4)
        self.narrow = nn.Linear(width * 4, width)

    def split(self, x):
        batch, length, width = x.shape
        return x.view(batch, length, self.heads, width // self.heads).transpose(1, 2)

    def forward(self, x):
        batch, length, width = x.shape
        y = self.norm1(x)
        mixed = F.scaled_dot_product_attention(
            self.split(self.query(y)), self.split(self.key(y)), self.split(self.value(y)), is_causal=True
        )
        x = x + self.output(mixed.transpose(1, 2).reshape(batch, length, width))
        return x + self.narrow(F.gelu(self.wide(self.norm2(x))))


class Model(nn.Module):
    def __init__(self, layers, width, words, context, heads=4):
        super().__init__()
        self.tokens = nn.Embedding(words, width)
        self.places = nn.Embedding(context, width)
        self.blocks = nn.ModuleList(Block(width, heads) for _ in range(layers))
        self.last = nn.LayerNorm(width)
        self.head = nn.Linear(width, words)

    def forward(self, inputs, where):
        hidden = self.tokens(inputs) + self.places(where)
        for block in self.blocks:
            hidden = block(hidden)
        return self.head(self.last(hidden))


def transformer_train(layers, width, steps, device):
    words, context, batch = 65, 128, 32
    model = Model(layers, width, words, context).to(device)
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.001, weight_decay=0.01)
    where = torch.arange(context, device=device)
    offsets = (torch.rand(batch, 1, device=device) * 65.0).long()
    inputs = (where.view(1, context) + offsets) % words
    wanted = ((where.view(1, context) + offsets + 1) % words).reshape(batch * context)
    start = now()
    first = 0.0
    loss_now = 0.0
    for step in range(steps + 1):
        logits = model(inputs, where).view(batch * context, words)
        loss = F.cross_entropy(logits, wanted)
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        if step == 0:
            loss_now = loss.item()
            first = now() - start
            start = now()
        if step == steps:
            loss_now = loss.item()
    print(f"transformer_train {layers}x{width} {now() - start:.0f} {loss_now} {first:.0f}")


class Residual(nn.Module):
    def __init__(self, channels):
        super().__init__()
        self.one = nn.Conv2d(channels, channels, 3, 1, 1)
        self.norm1 = nn.BatchNorm2d(channels)
        self.two = nn.Conv2d(channels, channels, 3, 1, 1)
        self.norm2 = nn.BatchNorm2d(channels)

    def forward(self, x):
        inner = F.relu(self.norm1(self.one(x)))
        return F.relu(x + self.norm2(self.two(inner)))


class Cnn(nn.Module):
    def __init__(self):
        super().__init__()
        self.stem = nn.Conv2d(3, 32, 3, 1, 1)
        self.norm1 = nn.BatchNorm2d(32)
        self.block = Residual(32)
        self.widen = nn.Conv2d(32, 64, 3, 1, 1)
        self.norm2 = nn.BatchNorm2d(64)
        self.head = nn.Linear(64, 10)

    def forward(self, x):
        first = self.block(F.relu(self.norm1(self.stem(x))))
        half = F.max_pool2d(first, 2, 2)
        second = F.max_pool2d(F.relu(self.norm2(self.widen(half))), 2, 2)
        return self.head(second.flatten(2).mean(2))


def cnn_train(rows, side, steps, device):
    batch = 64
    x = torch.randn(rows, 3, side, side, device=device)
    labels = (x.view(rows, -1) @ torch.randn(3 * side * side, 10, device=device)).argmax(1)
    model = Cnn().to(device)
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.001, weight_decay=0.01)
    start = now()
    first = 0.0
    last = 0.0
    for step in range(steps + 1):
        at = (step * batch) % (rows - batch + 1)
        loss = F.cross_entropy(model(x[at : at + batch]), labels[at : at + batch])
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        if step == 0:
            last = loss.item()
            first = now() - start
            start = now()
        if step == steps:
            last = loss.item()
    print(f"cnn_train {steps} {now() - start:.0f} {last} {first:.0f}")


def gpt_infer(layers, width, heads, reps, device):
    words, context, batch = 50257, 256, 8
    model = Model(layers, width, words, context, heads).to(device)
    where = torch.arange(context, device=device)
    inputs = (torch.rand(batch, context, device=device) * 50257.0).long()
    start = now()
    first = 0.0
    total = 0
    with torch.no_grad():
        for rep in range(reps + 1):
            hidden = model.tokens(inputs) + model.places(where)
            for block in model.blocks:
                hidden = block(hidden)
            final = model.last(hidden[:, context - 1, :])
            total = model.head(final).argmax(1).sum().item()
            if rep == 0:
                first = now() - start
                start = now()
    print(f"gpt_infer {layers}x{width} {now() - start:.0f} {total} {first:.0f}")


def monte_carlo(paths, reps, device):
    spot, strike, rate, spread = 100.0, 105.0, 0.05, 0.2
    start = now()
    first = 0.0
    price = 0.0
    for rep in range(reps + 1):
        z = torch.randn(paths, device=device)
        end = (z * spread + (rate - 0.5 * spread * spread)).exp() * spot
        price = ((end - strike).relu().mean() * math.exp(-rate)).item()
        if rep == 0:
            first = now() - start
            start = now()
    print(f"monte_carlo {paths} {now() - start:.0f} {price} {first:.0f}")


def sync(device):
    if device.type == "cuda":
        torch.cuda.synchronize()
    if device.type == "mps":
        torch.mps.synchronize()


def main():
    kind = sys.argv[1] if len(sys.argv) > 1 else "cpu"
    small = "small" in sys.argv[2:]
    if kind == "mps" and not torch.backends.mps.is_available():
        print("torch has no mps device here")
        return
    if kind == "cuda" and not torch.cuda.is_available():
        print("torch has no cuda device here")
        return
    device = torch.device(kind)
    torch.manual_seed(1)
    print(f"device torch {kind}")
    if small:
        mlp_train(2048, 5, device)
        mlp_infer(512, 2, device)
        kmeans(4096, 3, device)
        conjugate_gradient(128, 20, device)
        transformer_train(1, 32, 3, device)
        monte_carlo(100000, 2, device)
        cnn_train(256, 8, 3, device)
        gpt_infer(1, 32, 4, 2, device)
        return
    mlp_train(60000, 500, device)
    mlp_infer(65536, 20, device)
    kmeans(200000, 20, device)
    conjugate_gradient(4096, 100, device)
    transformer_train(4, 256, 20, device)
    monte_carlo(10000000, 20, device)
    cnn_train(4096, 32, 50, device)
    gpt_infer(12, 768, 12, 10, device)


if __name__ == "__main__":
    main()
