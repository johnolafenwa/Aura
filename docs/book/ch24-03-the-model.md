# The model and its training

We have the hard part. A transformer is attention plus a few plain pieces
we already know, stacked in a fixed pattern. Let's go through the pieces
in the order the data meets them, then put the whole program together and
train it.

## Vectors for tokens and for places

The model's input is a table of token numbers. The first step turns each
number into a vector, and that is an `Embedding`: a trainable table with
one row of 64 numbers for each of the 65 characters. Training moves the
rows so that characters that behave alike, such as the vowels, end up with
similar vectors.

Attention has a blind spot that the second embedding fixes. A weighted
average does not care about order: the same vectors in a different order
give the same average. But `on` and `no` are different words. So we add,
to each token's vector, a second vector that depends only on its place in
the window: one trainable row for place 0, one for place 1, up to place
63. After the addition, each vector says both what the character is and
where it is.

## One block

```aura fragment
class Block:
    norm1: LayerNorm
    attention: Attention
    norm2: LayerNorm
    wide: Linear
    narrow: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        attended = x + self.attention.forward(self.norm1.forward(x))
        return attended + self.narrow.forward(self.wide.forward(self.norm2.forward(attended)).gelu())

impl Module for Block
```

A block has two halves. The first is attention, where positions exchange
information. The second is a small network of the kind we built in
Chapter 23, applied to each position on its own: a layer that widens the
vector to four times its size, a `gelu`, and a layer that narrows it back.
If attention is where a position gathers what it needs, this half is where
it thinks about what it gathered.

Two details in `forward` are what make deep stacks of these blocks
trainable.

The first is the `x + ...` in both lines. Neither half replaces the vector
it is given. Each computes a change and adds it to its input. This is
called a *residual connection*. It gives the gradient a direct path from
the loss back to the earliest layers, and it means that a block that has
learned nothing yet passes its input through unharmed.

The second is `LayerNorm`. Before each half, the vector at each position
is shifted and scaled so that its numbers have a mean of zero and a spread
of one, with a trainable scale and offset applied after. Like the batch
norm of the last chapter, it keeps the numbers flowing through the network
at a steady size. Unlike batch norm, it looks at one position at a time,
so it behaves the same in training and in use and has no modes to switch.

## The whole model

```aura fragment
class Transformer:
    tokens: Embedding
    places: Embedding
    blocks: list[Block]
    norm: LayerNorm
    head: Linear

    def forward(self, ids: Tensor[int64]) -> Tensor[float32]:
        length = ids.shape()[1]
        mut hidden = self.tokens.forward(ids) + self.places.forward(tensor.arange(length, ids.device()))
        for block in self.blocks:
            hidden = block.forward(hidden)
        return self.head.forward(self.norm.forward(hidden))

impl Module for Transformer
```

`forward` reads top to bottom as the path of the data. Token numbers of
shape `[rows, length]` become vectors of shape `[rows, length, 64]`. The
place vectors have shape `[length, 64]` and broadcast across the rows. The
blocks run one after another, each improving the vectors. A last
`LayerNorm` tidies them, and `head` turns each position's vector into 65
scores, one for each character that might come next. The output has the
same shape as the table model's in Listing 24-2, `[rows, length, 65]`, so
`loss_of` and `batch` work unchanged.

Each class is followed by `impl Module`, so `model.parameters()` finds
every weight in every block, including the blocks inside the list.

## The program

Here is all of it. The top is the vocabulary from Listing 24-1, the middle
is the model, and the bottom is a training loop that would not be out of
place in Chapter 22.

*Filename: src/main.au*

```aura
import fs
import random
import tensor
from tensor import AdamW, Device, Embedding, LayerNorm, Linear, Module, Tensor

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

class Attention:
    query: Linear
    key: Linear
    value: Linear
    output: Linear
    heads: int64

    def split(self, x: Tensor[float32]) -> Tensor[float32]:
        # [rows, length, width] to [rows, heads, length, width / heads]
        shape = x.shape()
        return x.reshape([shape[0], shape[1], self.heads, shape[2] // self.heads]).permute([0, 2, 1, 3])

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        shape = x.shape()
        length = shape[1]
        q = self.split(self.query.forward(x))
        k = self.split(self.key.forward(x))
        v = self.split(self.value.forward(x))
        width = (shape[2] // self.heads) as float64
        scores = q.matmul(k.permute([0, 1, 3, 2])) * (1.0 / width.sqrt())
        places = tensor.arange(length, x.device())
        earlier = places.reshape([length, 1]).greater_equal(places.reshape([1, length]))
        weights = tensor.where(earlier, scores, scores.scalar(-1.0e30)).softmax()
        mixed = weights.matmul(v)
        return self.output.forward(mixed.permute([0, 2, 1, 3]).reshape(shape))

impl Module for Attention

class Block:
    norm1: LayerNorm
    attention: Attention
    norm2: LayerNorm
    wide: Linear
    narrow: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        attended = x + self.attention.forward(self.norm1.forward(x))
        return attended + self.narrow.forward(self.wide.forward(self.norm2.forward(attended)).gelu())

impl Module for Block

class Transformer:
    tokens: Embedding
    places: Embedding
    blocks: list[Block]
    norm: LayerNorm
    head: Linear

    def forward(self, ids: Tensor[int64]) -> Tensor[float32]:
        length = ids.shape()[1]
        mut hidden = self.tokens.forward(ids) + self.places.forward(tensor.arange(length, ids.device()))
        for block in self.blocks:
            hidden = block.forward(hidden)
        return self.head.forward(self.norm.forward(hidden))

impl Module for Transformer

def build(words: int64, context: int64, width: int64, heads: int64, layers: int64, device: Device) -> Transformer:
    mut blocks: list[Block] = []
    for layer in range(layers):
        seed = 100 + layer * 20
        blocks.append(Block(
            norm1=LayerNorm(width, device),
            attention=Attention(
                query=Linear(width, width, seed, device),
                key=Linear(width, width, seed + 2, device),
                value=Linear(width, width, seed + 4, device),
                output=Linear(width, width, seed + 6, device),
                heads=heads,
            ),
            norm2=LayerNorm(width, device),
            wide=Linear(width, 4 * width, seed + 8, device),
            narrow=Linear(4 * width, width, seed + 10, device),
        ))
    return Transformer(
        tokens=Embedding(words, width, 1, device),
        places=Embedding(context, width, 2, device),
        blocks=blocks,
        norm=LayerNorm(width, device),
        head=Linear(width, words, 3, device),
    )

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

def as_tensor(ids: list[int64], device: Device) -> Tensor[int64]:
    return Tensor[int64].from_array(Array[int64].from_list(ids, [ids.len()]), device)

def train(model: mut Transformer, data: Tensor[int64], held: Tensor[int64], context: int64, steps: int64) -> None:
    mut optimizer = AdamW(model.parameters(), lr=0.003)
    mut rng = random.Rng(1)
    mut held_rng = random.Rng(2)
    for step in range(steps + 1):
        inputs, targets = batch(data, 32, context, rng)
        loss = loss_of(model.forward(inputs), targets)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 250 == 0:
            tensor.set_grad_enabled(false)
            held_inputs, held_targets = batch(held, 256, context, held_rng)
            held_loss = loss_of(model.forward(held_inputs), held_targets)
            tensor.set_grad_enabled(true)
            print(f"step {step}: training {loss.item():.3f} held-out {held_loss.item():.3f}")

def save_model(model: Transformer, path: str) -> Result[int64, str]:
    state = model.state_dict()
    mut names: list[str] = []
    mut tensors: list[Tensor[float32]] = []
    for name in state.keys():
        view value = state[name]
        tensors.append(value.clone())
        names.append(name.clone())
    return tensor.save(path, names, tensors)

def main() -> int32:
    device = Device.cpu()
    context = 64
    match own fs.read_to_string("input.txt"):
        case Err(error):
            print(f"cannot read input.txt: {error}")
            return 1
        case Ok(text):
            vocabulary = build_vocabulary(text)
            ids = vocabulary.encode(text)
            split = ids.len() * 9 // 10
            mut model = build(vocabulary.letters.len(), context, 64, 4, 2, device)
            mut count = 0
            for name, param in model.named_parameters():
                count += param.value().len()
            print(f"{count} parameters")
            train(model, as_tensor(ids[:split], device), as_tensor(ids[split:], device), context, 1500)
            match save_model(model, "shakespeare.safetensors"):
                case Ok(bytes):
                    print(f"saved {bytes} bytes")
                case Err(message):
                    print(f"could not save: {message}")
                    return 1
    return 0
```

<span class="caption">Listing 24-4: A character transformer, trained on Shakespeare and saved to a file</span>

This one takes a while. On our machine, a four-core cloud computer with no
GPU, it runs for about ten minutes. It is a good moment for a cup of tea.

```console
$ aura run --backend direct src/main.au
112577 parameters
step 0: training 4.327 held-out 3.804
step 250: training 2.329 held-out 2.341
step 500: training 2.055 held-out 2.063
step 750: training 1.825 held-out 1.940
step 1000: training 1.749 held-out 1.863
step 1250: training 1.687 held-out 1.838
step 1500: training 1.548 held-out 1.805
saved 453604 bytes
```

`build` makes the model: two blocks, vectors of 64 numbers, four heads. It
gives every layer its own seed, as we have done since Chapter 21, so that
the run is repeatable. You will get the numbers above.

The model has 112,577 parameters. It is worth being able to account for a
number like that. The two embeddings are 65 × 64 and 64 × 64, which is
8,256. In a block, the four attention layers are 64 × 64 weights and 64
biases each, 16,640 together; the wide layer is 64 × 256 + 256 and the
narrow one 256 × 64 + 64, another 33,088; and two layer norms are 128
each. That makes 49,984 for a block and 99,968 for two. The last norm adds
128 and the head 64 × 65 + 65. The total is 112,577. Most of a
transformer's parameters are in its blocks, and that only becomes more
true as models grow.

Now the losses. Step 0 is 4.33, a little above the 4.17 of even guessing
that we worked out before, because random weights don't produce exactly
even scores. By step 250 the model has passed the 2.5 that the table
of pairs could reach, and it keeps going. The held-out loss ends near
1.8. Cross-entropy is a logarithm, so the difference from 2.5 is larger
than it looks: the table gave the right next character a chance of about 1
in 12 on average, and this model gives it about 1 in 6.

The held-out loss is above the training loss and the gap is growing
slowly. (The training loss is measured on one batch of 32 windows, so it
jumps around more than the held-out loss, which uses 256.) That is the start of the overfitting we met in Chapter 22,
and it is mild here because a million characters is a lot of data for a
model this small. Both numbers were still falling when we stopped. More
steps, or a wider model, would do better, and you can try both by changing
the arguments of `build` and `train`.

`train` turns gradients off around the held-out check with
`tensor.set_grad_enabled(false)`. The check never calls `backward()`, so
recording the steps for it would be wasted work.

The program ends by saving the weights with the `save_model` function of
Chapter 23. The ten minutes are now in a file of 454 kilobytes,
and we won't have to spend them again. In the next section we load the
file and make the model write.
