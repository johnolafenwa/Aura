# The weights

When we saved our Shakespeare model, we wrote its `state_dict` to a file
in the safetensors format: a list of names, and for each name a shape and
the numbers. The format is not Aura's own. It is the common format for
publishing trained models, and the copy of GPT-2 on the Hugging Face model
hub is in it. So `tensor.load_float32`, the same function we used to read
our own file, can read OpenAI's.

## Getting the files

Make a package and download three files into it. The first holds the
weights and is 548 MB. The other two belong to the tokenizer, and we'll
need them in the next section.

```console
$ aura new gpt2
$ cd gpt2
$ curl -L -o model.safetensors https://huggingface.co/openai-community/gpt2/resolve/main/model.safetensors
$ curl -L -o vocab.json https://huggingface.co/openai-community/gpt2/resolve/main/vocab.json
$ curl -L -o merges.txt https://huggingface.co/openai-community/gpt2/resolve/main/merges.txt
```

## Looking inside

Before writing any model code, let's ask the file what it contains.
`tensor.weight_names` lists the names in a safetensors file, and
`load_float32` reads one tensor by name.

*Filename: src/main.au*

```aura
import tensor
from tensor import Device

def main() -> int32:
    device = Device.cpu()
    path = "model.safetensors"
    match own tensor.weight_names(path):
        case Err(message):
            print(f"cannot read {path}: {message}")
            return 1
        case Ok(names):
            print(f"{names.len()} tensors")
            mut total = 0
            for name in sorted(names):
                match own tensor.load_float32(path, name, device):
                    case Err(message):
                        print(f"cannot load {name}: {message}")
                        return 1
                    case Ok(weights):
                        if name.ends_with(".attn.bias"):
                            continue
                        total += weights.len()
                        if not name.starts_with("h.") or name.starts_with("h.0."):
                            print(f"{name} {weights.shape()}")
            print(f"{total} parameters")
    return 0
```

<span class="caption">Listing 25-1: Listing the tensors in GPT-2's weight file</span>

```console
$ aura run src/main.au
160 tensors
h.0.attn.c_attn.bias [2304]
h.0.attn.c_attn.weight [768, 2304]
h.0.attn.c_proj.bias [768]
h.0.attn.c_proj.weight [768, 768]
h.0.ln_1.bias [768]
h.0.ln_1.weight [768]
h.0.ln_2.bias [768]
h.0.ln_2.weight [768]
h.0.mlp.c_fc.bias [3072]
h.0.mlp.c_fc.weight [768, 3072]
h.0.mlp.c_proj.bias [768]
h.0.mlp.c_proj.weight [3072, 768]
ln_f.bias [768]
ln_f.weight [768]
wpe.weight [1024, 768]
wte.weight [50257, 768]
124439808 parameters
```

The file has 160 tensors. We print the ones whose names don't start with
`h.`, and of those that do, only the ones for `h.0`, because `h.1` to
`h.11` repeat the same names.

If you have read Chapter 24, you can read this list. It is a description
of a model, and we have built every part of it:

- `wte.weight` is the token embedding: 50,257 tokens, each with a vector
  of 768 numbers. Our model had 65 tokens and vectors of 64.
- `wpe.weight` is the position embedding, with 1,024 rows. That is the
  context: GPT-2 can look back over 1,024 tokens, where we had 64
  characters.
- `h.0` to `h.11` are twelve blocks. Each has a layer norm `ln_1`, an
  attention `attn`, a second layer norm `ln_2`, and the widening and
  narrowing layers, here called `mlp.c_fc` and `mlp.c_proj`. The wide
  layer is 3,072, four times 768, the same ratio we used.
- `ln_f` is the last layer norm.

Two things differ from our model, and the shapes show both.

The attention has one layer, `c_attn`, of shape `[768, 2304]`, where ours
had three layers for the queries, keys, and values. 2,304 is three times
768. GPT-2 computes all three with one layer and cuts the result into
thirds. It's the same arithmetic, stored as one table.

And there is no `head`. The layer that turns the last vectors into scores
for 50,257 tokens is missing from the file. GPT-2 uses the token embedding
table a second time for that job, turned on its side. The vector for a
token on the way in is also the pattern that scores that token on the way
out. This is called *weight tying*, and it saves 38 million parameters.

The total is 124,439,808 parameters. We left out of the count twelve
tensors named `h.N.attn.bias`. Despite the name they aren't trained
weights. Each is a 1,024 by 1,024 table of ones and zeros: the causal mask
that we computed with `greater_equal` in Listing 24-3, stored in the file
by the program that saved it. We'll skip them when we load, and compute
the mask ourselves.

The weights have the shape `[inputs, outputs]`, for example `[768, 3072]`
for `mlp.c_fc`. That is how Aura's `Linear` stores its weight too, so they
will load as they are.

We now know the shape of the model exactly. Before we build it, we need
to be able to speak its language.
