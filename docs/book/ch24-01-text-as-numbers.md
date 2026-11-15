# Text as numbers

A model computes with tensors of numbers, and we have text. So the first
job is a rule for turning text into numbers and back. The pieces a rule
cuts text into are called *tokens*, and the rule is a *tokenizer*.

We'll use the simplest tokenizer there is: every character is a token. We
collect the different characters that appear in our text, sort them, and
number them from zero. Real models use a cleverer rule with pieces that are
often whole words, and we'll write that one in the next chapter. Characters
are enough to learn on, and they make the model's job harder in an
instructive way: it has to learn spelling too.

## The text

We need something to read. A file of about a million characters from
Shakespeare's plays has been the standard small example for years. Make a
package and download it:

```console
$ aura new shakespeare
$ cd shakespeare
$ curl -L -o input.txt https://raw.githubusercontent.com/karpathy/char-rnn/master/data/tinyshakespeare/input.txt
```

It begins like this, and goes on in the same form for 40,000 lines:

```text
First Citizen:
Before we proceed any further, hear me speak.

All:
Speak, speak.
```

## A vocabulary

*Filename: src/main.au*

```aura
import fs

class Vocabulary:
    letters: list[str]
    numbers: dict[str, int64]

    def encode(self, text: str) -> list[int64]:
        mut ids: list[int64] = []
        for letter in text.chars():
            ids.append(self.numbers[letter])
        return ids

    def decode(self, ids: list[int64]) -> str:
        mut text = ""
        for id in ids:
            text += self.letters[id]
        return text

def build_vocabulary(text: str) -> Vocabulary:
    mut seen = set[str]()
    for letter in text.chars():
        seen.add(letter.clone())
    letters = sorted(seen)
    mut numbers = dict[str, int64]()
    for at in range(letters.len()):
        numbers[letters[at].clone()] = at
    return Vocabulary(letters=letters, numbers=numbers)

def main() -> int32:
    match own fs.read_to_string("input.txt"):
        case Err(error):
            print(f"cannot read input.txt: {error}")
            return 1
        case Ok(text):
            vocabulary = build_vocabulary(text)
            print(f"{text.len()} characters, {vocabulary.letters.len()} different")
            print(vocabulary.letters[13:23])
            ids = vocabulary.encode("First Citizen")
            print(ids)
            print(vocabulary.decode(ids))
    return 0
```

<span class="caption">Listing 24-1: A vocabulary of characters, with `encode` and `decode`</span>

```console
$ aura run --backend direct src/main.au
aura: building native program...
1115394 characters, 65 different
[A, B, C, D, E, F, G, H, I, J]
[18, 47, 56, 57, 58, 1, 15, 47, 58, 47, 64, 43, 52]
First Citizen
```

There are 65 different characters in the file: the letters in both cases,
some punctuation, the space, and the line break. `encode` turns a string
into a list of their numbers, and `decode` turns a list of numbers back
into a string. `F` is 18, `i` is 47, and both of the `i`s in `Citizen` are
47 again. The numbers mean nothing in themselves. They are positions in a
sorted list.

Notice how we ran the program. The loops in this chapter go over a million
characters, and the default way `aura run` executes a program is built for
starting quickly, not for long loops in our own code. `--backend direct`
compiles the program to machine code first, as we saw in [Chapter
17](/book/ch17-01-run-and-build). On our machine this listing takes 14
seconds without the option and about one second with it. The results are
the same either way. We'll use the option for every program in this
chapter and the next, and leave the `building native program` line out of
the output we show from here on.

## What the model will learn

The task we'll train on is this: given some text, predict the next
character. That's all. It needs no labels made by a person, because the
text is its own answer key. For every position in the file, the right
answer is whatever character comes next.

One window of text gives many examples at once. Take the eight characters
`First Ci`. Given `F`, the answer is `i`. Given `Fi`, the answer is `r`.
Given `Fir`, it is `s`, and so on. If the inputs are a window of the text,
the targets are the same window moved one character to the right:

```text
inputs:   F i r s t   C i
targets:  i r s t   C i t
```

The length of the window is the *context*: the most characters the model
ever sees at once. We'll use 64.

A training batch, then, is a table of token numbers with one window per
row, and a second table of the same shape for the targets. We'll pick the
windows at random places in the text:

```aura fragment
def batch(data: Tensor[int64], rows: int64, context: int64, rng: mut random.Rng) -> (Tensor[int64], Tensor[int64]):
    # `rows` windows of `context` characters, and the same windows one step later.
    mut starts: list[int64] = []
    for row in range(rows):
        starts.append(rng.next_int(0, data.len() - context - 1))
    device = data.device()
    first = Tensor[int64].from_array(Array[int64].from_list(starts, [rows, 1]), device)
    places = first + tensor.arange(context, device).reshape([1, context])
    return (data.index_select(0, places), data.index_select(0, places + 1))
```

`data` is the whole text as one long `Tensor[int64]`. We draw a starting
place for each row and put the starts in a column of shape `[rows, 1]`.
Adding `arange(context)` as a row of shape `[1, context]` broadcasts the
two into a table of shape `[rows, context]` in which every row counts up
from its start. `index_select` with a table of places returns a table of
the values at those places, so the first call reads the windows and the
second, with every place moved up by one, reads the targets. No loop
touches the characters one at a time.

## The simplest model: a table of what follows what

Before building anything clever we want a baseline: the simplest model
that can do the task, and the loss it reaches. Then we'll know what the
clever model has to beat.

The simplest idea is to predict the next character from the current one
alone. After `q` comes `u`. After a line break comes a capital letter.
Such a model is a table with a row for each character, holding 65 scores,
one for each character that might follow. An `Embedding` is exactly a
table of trainable rows that we look up by number, so the whole model is
`Embedding(65, 65)`.

*Filename: src/main.au*

```aura
import fs
import random
import tensor
from tensor import AdamW, Device, Embedding, Tensor

class Vocabulary:
    letters: list[str]
    numbers: dict[str, int64]

    def encode(self, text: str) -> list[int64]:
        mut ids: list[int64] = []
        for letter in text.chars():
            ids.append(self.numbers[letter])
        return ids

    def decode(self, ids: list[int64]) -> str:
        mut text = ""
        for id in ids:
            text += self.letters[id]
        return text

def build_vocabulary(text: str) -> Vocabulary:
    mut seen = set[str]()
    for letter in text.chars():
        seen.add(letter.clone())
    letters = sorted(seen)
    mut numbers = dict[str, int64]()
    for at in range(letters.len()):
        numbers[letters[at].clone()] = at
    return Vocabulary(letters=letters, numbers=numbers)

def as_tensor(ids: list[int64], device: Device) -> Tensor[int64]:
    return Tensor[int64].from_array(Array[int64].from_list(ids, [ids.len()]), device)

def batch(data: Tensor[int64], rows: int64, context: int64, rng: mut random.Rng) -> (Tensor[int64], Tensor[int64]):
    # `rows` windows of `context` characters, and the same windows one step later.
    mut starts: list[int64] = []
    for row in range(rows):
        starts.append(rng.next_int(0, data.len() - context - 1))
    device = data.device()
    first = Tensor[int64].from_array(Array[int64].from_list(starts, [rows, 1]), device)
    places = first + tensor.arange(context, device).reshape([1, context])
    return (data.index_select(0, places), data.index_select(0, places + 1))

def loss_of(scores: Tensor[float32], targets: Tensor[int64]) -> Tensor[float32]:
    shape = scores.shape()
    return tensor.cross_entropy(scores.reshape([shape[0] * shape[1], shape[2]]), targets.reshape([shape[0] * shape[1]]))

def pick(scores: Tensor[float32], rng: mut random.Rng) -> int64:
    # Draws one character number, each with the chance the scores give it.
    chances = scores.softmax().to_array()
    mut left = rng.next_float()
    for at in range(chances.len()):
        left -= chances[at] as float64
        if left <= 0.0:
            return at
    return chances.len() - 1

def main() -> int32:
    device = Device.cpu()
    match own fs.read_to_string("input.txt"):
        case Err(error):
            print(f"cannot read input.txt: {error}")
            return 1
        case Ok(text):
            vocabulary = build_vocabulary(text)
            ids = vocabulary.encode(text)
            split = ids.len() * 9 // 10
            train = as_tensor(ids[:split], device)
            held = as_tensor(ids[split:], device)

            words = vocabulary.letters.len()
            mut table = Embedding(words, words, 1, device)
            mut optimizer = AdamW(table.parameters(), lr=0.01)
            mut rng = random.Rng(1)
            for step in range(2001):
                inputs, targets = batch(train, 32, 64, rng)
                loss = loss_of(table.forward(inputs), targets)
                optimizer.zero_grad()
                loss.backward()
                optimizer.step()
                if step % 500 == 0:
                    held_inputs, held_targets = batch(held, 256, 64, rng)
                    held_loss = loss_of(table.forward(held_inputs), held_targets)
                    print(f"step {step}: training {loss.item():.3f} held-out {held_loss.item():.3f}")

            mut out = vocabulary.encode("\n")
            for step in range(200):
                last = as_tensor([out[out.len() - 1]], device)
                out.append(pick(table.forward(last).reshape([-1]), rng))
            print(vocabulary.decode(out))
    return 0
```

<span class="caption">Listing 24-2: A model that predicts each character from the one before it</span>

```console
$ aura run --backend direct src/main.au
step 0: training 4.174 held-out 4.161
step 500: training 2.531 held-out 2.529
step 1000: training 2.431 held-out 2.486
step 1500: training 2.490 held-out 2.486
step 2000: training 2.452 held-out 2.496

Sharthyor s seavetllof p;
T:

IVI w! wiseit nn st S:
HA: ELENRUSLAnd l e s henoit g l ED:
I ips.
IShans ws outhitun, net mbeand dyond cre:rasm grecoougist m.
Gers my fre y: wout; ared ncr im, bit hy w
```

The first nine tenths of the text are for training and the last tenth is
held out, as in Chapter 22.

`table.forward(inputs)` looks up a row of 65 scores for every character of
every window, so its shape is `[32, 64, 65]`. `cross_entropy` wants a table
of scores with one row per example and a list of targets, so `loss_of`
flattens the batch and the positions together: 2,048 rows of 65 scores, and
2,048 targets. Every position of every window is one example.

Look at the first loss. Before any training the scores are small random
numbers, the model gives every character about the same chance, and
the loss for guessing evenly among 65 choices is the natural logarithm of
65, which is 4.17. That is what we see. It is a good habit to work out what
the loss should be at step 0 and check it; a different number means a
mistake in the model or the data.

Training brings the loss down to about 2.5 and it stays there. That is all
the knowledge a table of pairs can hold. The last lines of `main` use the
table to write text: start from a line break, look up the scores for the
last character, pick the next one, and repeat. `pick` does not take the
highest score. It turns the scores into chances with `softmax` and draws
one at random with those chances, so that likely characters are picked
often and unlikely ones sometimes. We'll come back to why in the last
section of this chapter.

The output is not English, but it is not noise either. The letters come in
plausible pairs, there are spaces at word-like distances, and there are
capitals after line breaks and colons at the ends of short lines, like the
speaker names in the file. Everything the model knows is which character
tends to follow which.

To do better, a prediction has to depend on more than one character. The
`h` after `t` should know whether a `w` came before the two of them, and a
speaker's name should know that a blank line came before it. Each
position has to look back at the others and take what it needs from them.
That is what attention does.
