"""GPT-2 with its real weights in PyTorch, as float32 and as float16.

    python3 gpt2.py model.safetensors cpu
    python3 gpt2.py model.safetensors cuda
    python3 gpt2.py model.safetensors cuda small

Each row prints `<name> <size> <milliseconds> <result> <first pass
milliseconds>`, the same rows as `gpt2.au`, and the tokens are the same
tokens.
"""

import sys
import time

import torch
import torch.nn.functional as F
from safetensors.torch import load_file


def now():
    return time.perf_counter() * 1000.0


def gelu_new(x):
    inner = (x + x * x * x * 0.044715) * 0.7978845608028654
    return x * 0.5 * (inner.tanh() + 1.0)


def linear(x, weights, name):
    # The file holds each weight as `[in, out]`.
    return x @ weights[f"{name}.weight"] + weights[f"{name}.bias"]


def layer_norm(x, weights, name):
    return F.layer_norm(x, x.shape[-1:], weights[f"{name}.weight"], weights[f"{name}.bias"])


def split_heads(x, heads):
    batch, length, width = x.shape
    return x.view(batch, length, heads, width // heads).transpose(1, 2)


def next_tokens(weights, layers, ids):
    batch, length = ids.shape
    heads = 12
    hidden = weights["wte.weight"][ids] + weights["wpe.weight"][:length]
    for layer in range(layers):
        normed = layer_norm(hidden, weights, f"h.{layer}.ln_1")
        query, key, value = linear(normed, weights, f"h.{layer}.attn.c_attn").split(hidden.shape[2], dim=2)
        mixed = F.scaled_dot_product_attention(
            split_heads(query, heads), split_heads(key, heads), split_heads(value, heads), is_causal=True
        )
        mixed = mixed.transpose(1, 2).reshape(hidden.shape)
        attended = hidden + linear(mixed, weights, f"h.{layer}.attn.c_proj")
        inner = layer_norm(attended, weights, f"h.{layer}.ln_2")
        wide = gelu_new(linear(inner, weights, f"h.{layer}.mlp.c_fc"))
        hidden = attended + linear(wide, weights, f"h.{layer}.mlp.c_proj")
    final = layer_norm(hidden[:, length - 1, :], weights, "ln_f")
    return (final @ weights["wte.weight"].T).argmax(-1)


def generate(name, weights, layers, count, device):
    tokens = [15496, 11, 314, 1101, 257, 3303, 2746, 11]
    start = now()
    first = 0.0
    for step in range(count + 1):
        ids = torch.tensor([tokens], device=device)
        tokens.append(next_tokens(weights, layers, ids).item())
        if step == 0:
            first = now() - start
            start = now()
    print(f"{name} {count} {now() - start:.0f} {sum(tokens)} {first:.0f}")
    print(f"# {name} tokens {tokens}")


def infer(name, weights, layers, batch, context, reps, device):
    places = torch.arange(batch * context, device=device)
    ids = ((places * 7919 + 104729) % 50257).view(batch, context)
    start = now()
    first = 0.0
    total = 0
    for rep in range(reps + 1):
        total = next_tokens(weights, layers, ids).sum().item()
        if rep == 0:
            first = now() - start
            start = now()
    print(f"{name} {batch}x{context} {now() - start:.0f} {total} {first:.0f}")


def main():
    path = next((arg for arg in sys.argv[1:] if arg.endswith(".safetensors")), None)
    if path is None:
        print("usage: gpt2.py <model.safetensors> [cpu|cuda|mps] [small]")
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
    print(f"device torch {kind}")
    layers, count, batch, context, reps = (2, 3, 2, 16, 2) if small else (12, 20, 8, 256, 10)
    weights = {
        name: value.to(device)
        for name, value in load_file(path).items()
        if not name.endswith(".attn.bias")
    }
    with torch.no_grad():
        generate("gpt2_generate", weights, layers, count, device)
        infer("gpt2_infer", weights, layers, batch, context, reps, device)
        halves = {name: value.half() for name, value in weights.items()}
        generate("gpt2_generate_float16", halves, layers, count, device)
        infer("gpt2_infer_float16", halves, layers, batch, context, reps, device)


main()
