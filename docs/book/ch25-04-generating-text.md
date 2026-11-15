# Generating text

We have a tokenizer that turns text into token numbers and back, and a
model that turns token numbers into scores for the next token. The
program we want is the loop from Chapter 24 with those two in place of
the character vocabulary and the small model.

## One program

Put the two listings into one file. Take the tokenizer of Listing 25-2
without its `main`, then the model of Listing 25-3 without its `main`,
and give the file these imports:

*Filename: src/main.au*

```aura
import fs
import json
import random
import regex
import sys
import tensor
from tensor import Device, Embedding, LayerNorm, Linear, Module, Tensor
```

Then add a function to choose the next token, and a new `main`:

```aura fragment
def pick(scores: Tensor[float32], top: int64, temperature: float64, rng: mut random.Rng) -> int64:
    # Draws among the `top` best-scoring tokens only.
    best = tensor.top_indices(scores, top)
    chances = (scores.index_select(0, best) / temperature).softmax().to_array()
    places = best.to_array()
    mut left = rng.next_float()
    for at in range(top):
        left -= chances[at] as float64
        if left <= 0.0:
            return places[at]
    return places[top - 1]
```

<span class="caption">Listing 25-4: Choosing the next token from the 40 best</span>

```aura fragment
def main() -> int32:
    device = Device.cpu()
    args = sys.args()
    mut prompt = "Hello, I'm a language model,"
    if args.len() > 0:
        prompt = args[0].clone()
    mut seed = 1
    if args.len() > 1:
        match parse_int64(args[1]):
            case Ok(number):
                seed = number
            case Err(error):
                print(f"the seed must be a whole number, not {args[1]}")
                return 1

    match own load_tokenizer("vocab.json", "merges.txt"):
        case Err(message):
            print(message)
            return 1
        case Ok(tokenizer):
            mut model = build(device)
            match load_weights(model, "model.safetensors", device):
                case Err(message):
                    print(f"could not load the weights: {message}")
                    return 1
                case Ok(count):
                    pass
            tensor.set_grad_enabled(false)

            mut tokens = tokenizer.encode(prompt)
            mut rng = random.Rng(seed)
            for step in range(40):
                next = pick(model.next_scores(tokens, device), 40, 0.8, rng)
                tokens.append(next)
            print(tokenizer.decode(tokens))
    return 0
```

<span class="caption">Listing 25-5: Reading a prompt, loading GPT-2, and writing 40 tokens</span>

`main` takes the prompt from the command line, with a default, and an
optional seed for the random choices. It loads the tokenizer and the
model, encodes the prompt, and runs the loop we know: get the scores for
the next token, pick one, append it. At the end it decodes all the
tokens, the prompt's and the model's, back into text.

```console
$ aura run --backend direct src/main.au
Hello, I'm a language model, so I've got some ideas for how you could teach languages how to make sense of it.

This is the first time I've looked up a list of the top 10 languages to learn.
```

That is GPT-2 writing, in a program we wrote from the tokenizer up. Give
it a prompt of your own after `--`, and a different seed for a different
continuation:

```console
$ aura run --backend direct src/main.au -- "The meaning of life is" 2
The meaning of life is not only to love the world but to live life for the people living to love it. The life of a person is not a matter of what one thinks of, but of what one sees.
```

```console
$ aura run --backend direct src/main.au -- "In a shocking finding, scientists discovered a herd of unicorns" 3
In a shocking finding, scientists discovered a herd of unicorns had been spotted in the same area.


As part of their study, the researchers conducted a study in which they conducted hundreds of animals in the wild in a remote remote mountain region.
```

The sentences are grammatical. The text stays on the subject of its
prompt, and each continuation matches its prompt's manner: the first is
chatty, the second sounds like a philosophy essay, the third like a
news report. It is also a model from 2019 with a habit of repeating
itself and no commitment to facts. There was no herd of unicorns.

Compare this with the Shakespeare model, which could barely spell. The
program is nearly the same. What differs is scale: twelve blocks where we
had two, vectors of 768 where we had 64, a context of 1,024 tokens that
are mostly whole words, and training on a large part of the web where we
had one file. Everything that looks like understanding here came from
predicting the next token, at a size where that requires modeling a great
deal about how text works.

## Top-k sampling

In Chapter 24 we drew the next character from all 65, with the chances
the model gave. With 50,257 tokens that has a flaw. Most tokens are
absurd in any given place, and the model gives each of them a tiny chance,
but there are so many that together they are picked every so often. One
absurd token is enough to throw the rest of the text off course.

`pick` uses the usual remedy, called *top-k sampling*: keep the 40
best-scoring tokens and draw among those only. `tensor.top_indices(scores,
40)` returns the places of the 40 largest scores, largest first.
`index_select` reads the scores at those places, and we apply the
temperature and `softmax` to those 40 alone, so their chances add up to
one. The draw is the same subtraction loop as before, and the token
returned is the place in the full vocabulary, from `places`.

The two knobs work together. A smaller `top` or a lower temperature makes
the text safer and more repetitive. Larger values make it more varied and
more likely to wander. We use 40 and 0.8, which are common choices for
GPT-2.

## Speed

On our four-core machine with no GPU, the program takes about two seconds
to build the model and load the weights, and then a little under a second
for each token.

Most of that second is avoidable, and it is worth knowing why. Every step
runs all twelve blocks over every token so far, although only one token
is new. Because attention is causal, the vectors of the earlier tokens
come out the same each time. Production systems keep the keys and values
computed for earlier tokens, a *key-value cache*, and run the blocks for
the new token alone, which makes each step cost about the same however
long the text is. Our loop is the simple version. Adding the cache is a
good exercise once you know the model well: `Attention.forward` would
take the saved keys and values, add the new token's to them with
`tensor.cat`, and return the longer tables.

The other way to go faster is a GPU, and that needs no change to the
model. It is the subject of the next chapter.

## Summary

Running a published model takes three things. The weights are a file of
named tensors, and listing the names and shapes tells you the model's
design. The tokenizer must cut text exactly as the model's training data
was cut; GPT-2's is byte pair encoding, which builds tokens by replaying
a recorded list of merges over the bytes of each word. The model is
written as classes whose field names spell out the names in the file, so
that `load_state_dict` can put each tensor in its place.

GPT-2 itself is the transformer of Chapter 24 at a larger size, with one
layer producing queries, keys, and values together, its own `gelu`, and
the token embedding reused as the output layer. We checked it by its
predictions, then joined it to the tokenizer and sampled from its 40 most
likely tokens to write text.

The larger GPT-2 sizes are on the same hub as `gpt2-medium`, `gpt2-large`,
and `gpt2-xl`. They use the same tokenizer files. To run one, read its
shapes with Listing 25-1 and change the numbers in `build`: the width, the
number of blocks, and the number of heads.
