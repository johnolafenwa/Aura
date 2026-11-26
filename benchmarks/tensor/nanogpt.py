"""The character model of `nanogpt.au` in PyTorch.

    python3 nanogpt.py input.txt cpu
    python3 nanogpt.py input.txt cuda
    python3 nanogpt.py input.txt cuda small

The row prints `<name> <size> <milliseconds> <last loss> <first step
milliseconds>`. The random numbers differ from Aura's, so a loss is
close to Aura's, not equal to it.
"""

import sys
import time

import torch
import torch.nn.functional as F
from torch import nn


def now():
    return time.perf_counter() * 1000.0


class Block(nn.Module):
    def __init__(self, width, heads):
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


class Transformer(nn.Module):
    def __init__(self, words, context, width, heads, layers):
        super().__init__()
        self.tokens = nn.Embedding(words, width)
        self.places = nn.Embedding(context, width)
        self.blocks = nn.ModuleList(Block(width, heads) for _ in range(layers))
        self.norm = nn.LayerNorm(width)
        self.head = nn.Linear(width, words)

    def forward(self, ids):
        hidden = self.tokens(ids) + self.places(torch.arange(ids.shape[1], device=ids.device))
        for block in self.blocks:
            hidden = block(hidden)
        return self.head(self.norm(hidden))


def batch_of(data, rows, context, generator):
    starts = torch.randint(0, len(data) - context - 1, (rows, 1), generator=generator).to(data.device)
    places = starts + torch.arange(context, device=data.device).view(1, context)
    return data[places], data[places + 1]


def loss_of(scores, targets):
    return F.cross_entropy(scores.view(-1, scores.shape[2]), targets.reshape(-1))


def main():
    path = next((arg for arg in sys.argv[1:] if arg.endswith(".txt")), None)
    if path is None:
        print("usage: nanogpt.py <input.txt> [cpu|cuda|mps] [small]")
        return
    kind = next((arg for arg in sys.argv[1:] if arg in ("cpu", "cuda", "mps")), "cpu")
    small = "small" in sys.argv[1:]
    if kind == "mps" and not torch.backends.mps.is_available():
        print("torch has no mps device here")
        return
    if kind == "cuda" and not torch.cuda.is_available():
        print("torch has no cuda device here")
        return
    device = torch.device(kind)
    torch.manual_seed(1)
    print(f"device torch {kind}")
    layers, heads, width, context, rows, steps = (1, 4, 32, 32, 8, 3) if small else (6, 6, 384, 256, 64, 200)
    with open(path, encoding="utf-8") as file:
        text = file.read()
    letters = sorted(set(text))
    numbers = {letter: at for at, letter in enumerate(letters)}
    ids = torch.tensor([numbers[letter] for letter in text], device=device)
    split = len(ids) * 9 // 10
    data, held = ids[:split], ids[split:]
    model = Transformer(len(letters), context, width, heads, layers).to(device)
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.001, weight_decay=0.01)
    generator = torch.Generator().manual_seed(1)
    start = now()
    first = 0.0
    last = 0.0
    for step in range(steps + 1):
        inputs, targets = batch_of(data, rows, context, generator)
        loss = loss_of(model(inputs), targets)
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        if step == 0:
            last = loss.item()
            first = now() - start
            start = now()
        if step == steps:
            last = loss.item()
    print(f"nanogpt_train {layers}x{width} {now() - start:.0f} {last} {first:.0f}")
    with torch.no_grad():
        held_inputs, held_targets = batch_of(held, rows, context, torch.Generator().manual_seed(2))
        print(f"# nanogpt_train held-out loss {loss_of(model(held_inputs), held_targets).item()}")


main()
