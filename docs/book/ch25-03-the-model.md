# The model

The weight file told us what GPT-2 is made of: two embeddings, twelve
blocks, a final layer norm. We built all of those in Chapter 24. This
section builds them again to GPT-2's measurements, and with one new
constraint: the weights must find their places.

## Names are the contract

Recall how `load_state_dict` works. A class with `impl Module` names each
of its parameters by the path of fields that leads to it. A `Linear` in a
field called `hidden` has parameters named `hidden.weight` and
`hidden.bias`, and a block at index 3 of a list in a field called `h`
contributes names starting with `h.3.`. Loading matches the names in the
state against the names in the model and refuses if either side has a
name the other lacks.

The file calls its tensors `wte.weight`, `h.0.attn.c_attn.weight`, and so
on. So if we name our fields `wte`, `h`, `attn`, and `c_attn`, and nest
the classes the same way, the names will agree and the weights will load
without any translation table. In Chapter 24 we chose names that read
well. Here we use OpenAI's, which are terse, and we'll say what each one
is.

*Filename: src/main.au*

```aura
import tensor
from tensor import Device, Embedding, LayerNorm, Linear, Module, Tensor

def gelu_new(x: Tensor[float32]) -> Tensor[float32]:
    # The approximation of gelu that GPT-2 was trained with.
    inner = (x + x * x * x * 0.044715) * 0.7978845608028654
    return x * 0.5 * (inner.tanh() + 1.0)

def split_heads(x: Tensor[float32], heads: int64) -> Tensor[float32]:
    shape = x.shape()
    return x.reshape([shape[0], shape[1], heads, shape[2] // heads]).permute([0, 2, 1, 3])

class Attention:
    c_attn: Linear
    c_proj: Linear
    heads: int64

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        shape = x.shape()
        width = shape[2]
        joined = self.c_attn.forward(x)
        query = split_heads(joined.slice(2, 0, width), self.heads)
        key = split_heads(joined.slice(2, width, 2 * width), self.heads)
        value = split_heads(joined.slice(2, 2 * width, 3 * width), self.heads)
        mixed = tensor.scaled_dot_product_attention(query, key, value, true)
        return self.c_proj.forward(mixed.permute([0, 2, 1, 3]).reshape(shape))

impl Module for Attention

class Mlp:
    c_fc: Linear
    c_proj: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        return self.c_proj.forward(gelu_new(self.c_fc.forward(x)))

impl Module for Mlp

class Block:
    ln_1: LayerNorm
    attn: Attention
    ln_2: LayerNorm
    mlp: Mlp

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        attended = x + self.attn.forward(self.ln_1.forward(x))
        return attended + self.mlp.forward(self.ln_2.forward(attended))

impl Module for Block

class Gpt2:
    wte: Embedding
    wpe: Embedding
    h: list[Block]
    ln_f: LayerNorm

    def next_scores(self, tokens: list[int64], device: Device) -> Tensor[float32]:
        # A score for each of the 50,257 tokens that could come next.
        length = tokens.len()
        ids = Tensor[int64].from_array(Array[int64].from_list(tokens, [1, length]), device)
        mut hidden = self.wte.forward(ids) + self.wpe.forward(tensor.arange(length, device))
        for block in self.h:
            hidden = block.forward(hidden)
        last = self.ln_f.forward(hidden.slice(1, length - 1, length)).reshape([1, -1])
        return last.matmul(self.wte.weight.value().transpose()).reshape([-1])

impl Module for Gpt2

def build(device: Device) -> Gpt2:
    width = 768
    mut blocks: list[Block] = []
    for layer in range(12):
        blocks.append(Block(
            ln_1=LayerNorm(width, device),
            attn=Attention(c_attn=Linear(width, 3 * width, 1, device), c_proj=Linear(width, width, 1, device), heads=12),
            ln_2=LayerNorm(width, device),
            mlp=Mlp(c_fc=Linear(width, 4 * width, 1, device), c_proj=Linear(4 * width, width, 1, device)),
        ))
    return Gpt2(
        wte=Embedding(50257, width, 1, device),
        wpe=Embedding(1024, width, 1, device),
        h=blocks,
        ln_f=LayerNorm(width, device),
    )

def load_weights(model: mut Gpt2, path: str, device: Device) -> Result[int64, str]:
    mut state = dict[str, Tensor[float32]]()
    for name in try tensor.weight_names(path):
        if name.ends_with(".attn.bias"):
            continue
        state[name.clone()] = try tensor.load_float32(path, name, device)
    return model.load_state_dict(state)

def main() -> int32:
    device = Device.cpu()
    mut model = build(device)
    match load_weights(model, "model.safetensors", device):
        case Err(message):
            print(f"could not load the weights: {message}")
            return 1
        case Ok(count):
            print(f"loaded {count} tensors")
    tensor.set_grad_enabled(false)

    # "Hello, I'm a language model,"
    mut tokens = [15496, 11, 314, 1101, 257, 3303, 2746, 11]
    scores = model.next_scores(tokens, device)
    chances = scores.softmax()
    best = tensor.top_indices(scores, 5).to_array()
    for at in range(5):
        token = best[at]
        print(f"token {token}: {chances.slice(0, token, token + 1).item() * 100.0:.1f}%")

    for step in range(10):
        next = model.next_scores(tokens, device).argmax().item()
        tokens.append(next)
    print(tokens)
    return 0
```

<span class="caption">Listing 25-3: GPT-2, with its weights loaded, predicting the next token</span>

```console
$ aura run --backend direct src/main.au
loaded 148 tensors
token 407: 16.4%
token 290: 15.2%
token 314: 10.7%
token 523: 7.4%
token 475: 7.2%
[15496, 11, 314, 1101, 257, 3303, 2746, 11, 407, 257, 8300, 3303, 13, 314, 1101, 257, 3303, 2746]
```

## What is different from Chapter 24

Put this listing next to Listing 24-4 and it is the same model. `Block`
is identical apart from its field names: `ln_1` and `ln_2` are the layer
norms, `attn` is the attention, and `mlp` holds the wide layer `c_fc` and
the narrow layer `c_proj`. `Gpt2` has the token embedding `wte`, the
position embedding `wpe`, the list of blocks `h`, and the last norm
`ln_f`. There are four real differences.

The first is one layer for queries, keys, and values. `c_attn` produces 2,304
numbers for each position, and `forward` cuts them into three runs of 768
with `slice` along the last axis. Each third is then split into 12 heads
as before.

The second is the built-in attention. In Chapter 24 we wrote out the scores, the
mask, and the softmax. `tensor.scaled_dot_product_attention` computes
exactly those lines, and its last argument, `true`, asks for the causal
mask. Having written it once by hand, we can use the library version
knowing what it does.

The third is a different `gelu`. GPT-2 was trained with an approximation of it
built from `tanh`. The exact function, which `x.gelu()` computes, differs
from it by less than a thousandth, but the weights were trained
with the approximation, so we use it too. `gelu_new` is that formula
written with tensor arithmetic. This is a general point about running
someone else's model: small numerical choices are part of the model.

The fourth is the missing head. The last line of `next_scores` multiplies the final vector
by `self.wte.weight.value().transpose()`: the token embedding table,
turned on its side, as we predicted from the file. One more saving is
possible here because we are not training. The model computes a vector
for every position, but only the last position predicts the token we
want. So we `slice` out the last position before the final layer norm,
and do the large multiplication against 50,257 token vectors for one
position where training would need all of them.

## Building and loading

`build` makes a model of the right shape. Every layer gets the seed 1
because the random starting weights are about to be replaced, so it does
not matter what they are.

`load_weights` reads each tensor in the file into a dictionary and hands
the dictionary to `load_state_dict`. It skips the names ending in
`.attn.bias`, which are the stored masks we met in the first section.
Without that line the load fails, because the model has no parameters by
those names: `load_state_dict` reports that it expects 148 tensors and
found 160. The program reports 148 tensors
loaded: the 160 in the file less the 12 masks. Every one of the model's
parameters was matched by name and shape with a tensor from the file.

After loading we call `tensor.set_grad_enabled(false)`. We are only going
to run the model, and recording every operation for a `backward()` that
never comes would cost time and a great deal of memory at this size.

## Does it work?

A model with the wrong weights, or the right weights in the wrong places,
still prints numbers. We need evidence that this is GPT-2 and not an
expensive random number generator.

The program gives the model the eight tokens of `Hello, I'm a language
model,` that our tokenizer produced in the last section. It prints the
five tokens the model thinks most likely to come next, with their
chances. `top_indices` returns the places of the largest scores. Then it
extends the text ten times, each time taking the single most likely
token.

Look up the five numbers in `vocab.json` and they are ` not`, ` and`,
` I`, ` so`, and ` but`: five reasonable ways to go on after "I'm a
language model," and no unreasonable ones, out of 50,257 choices. The ten
tokens that follow, decoded, read ` not a programming language. I'm a
language model`. That is grammatical English, on the subject of the
prompt. A model with a single misplaced weight table does not do that.

The chances are worth a look too. The best choice has 16%, and the top
five together have a little over half. The model is not sure what comes
next, and it should not be, because many continuations are sensible. That
spread is what we'll sample from.

Reading tokens as numbers is no way to live. In the last section we join
the tokenizer to the model.
