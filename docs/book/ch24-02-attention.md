# Attention

Picture the model partway through a line of text. Each position holds a
list of numbers, a vector, that describes the character there. We want each
position to improve its vector using the positions before it. The question
is how a position decides which of the earlier ones matter.

Attention answers with three trainable layers. Each position computes
three things from its own vector:

- a *query*, which describes what this position is looking for,
- a *key*, which describes what this position has to offer,
- a *value*, which is what it hands over if it is chosen.

To decide how much position 5 should take from position 2, we compare the
query of 5 with the key of 2 by multiplying them element by element and
adding up the results. A large sum means a good match. Doing that for
every pair gives a table of scores, `softmax` turns each row of the table
into weights that add up to one, and each position's new vector is the
weighted average of all the values. Because the three layers are trained,
the model itself learns what to look for and what to offer.

That paragraph is the whole mechanism. Let's compute it for a sequence of
five positions with vectors of eight numbers, and print the tables.

*Filename: src/main.au*

```aura
import tensor
from tensor import Device, Linear, Tensor

def show(table: Tensor[float32]) -> None:
    values = table.to_array()
    for row in range(table.shape()[0]):
        mut line = ""
        for column in range(table.shape()[1]):
            line += f" {values[row, column]:5.2f}"
        print(line)

def main():
    device = Device.cpu()
    length = 5
    width = 8
    x = tensor.randn([length, width], 1, device)

    query = Linear(width, width, 2, device)
    key = Linear(width, width, 3, device)
    value = Linear(width, width, 4, device)

    q = query.forward(x)
    k = key.forward(x)
    scores = q.matmul(k.transpose()) * (1.0 / (width as float64).sqrt())
    print("scores")
    show(scores)

    places = tensor.arange(length, device)
    earlier = places.reshape([length, 1]).greater_equal(places.reshape([1, length]))
    weights = tensor.where(earlier, scores, scores.scalar(-1.0e30)).softmax()
    print("weights")
    show(weights)

    mixed = weights.matmul(value.forward(x))
    print(mixed.shape())
```

<span class="caption">Listing 24-3: Attention over five positions, one step at a time</span>

```console
$ aura run src/main.au
scores
 -0.57 -0.79 -0.87 -0.33  0.43
  0.09 -0.35 -0.09 -0.44  0.31
 -0.39 -1.40 -0.62 -0.37  0.82
 -0.35  0.30  0.17  0.02 -0.06
 -0.19 -0.17 -0.38 -0.19 -0.11
weights
  1.00  0.00  0.00  0.00  0.00
  0.61  0.39  0.00  0.00  0.00
  0.46  0.17  0.37  0.00  0.00
  0.17  0.32  0.28  0.24  0.00
  0.20  0.21  0.17  0.20  0.22
[5, 8]
```

`x` stands in for the five vectors, one per row. `q` and `k` have the same
shape, `[5, 8]`. The line that computes `scores` does every comparison at
once: `q.matmul(k.transpose())` multiplies each row of `q` with each row of
`k`, which gives a `[5, 5]` table whose entry in row 3, column 1 is the
match between the query of position 3 and the key of position 1. Dividing
by the square root of the width keeps the scores at a moderate size
however wide the vectors are, which keeps `softmax` from putting all its
weight on one entry early in training.

## No looking ahead

The next three lines matter more than they look. Our model is trained to
predict the next character. If position 2 could take information from
position 3, it would be reading the answer. So a position may attend only
to itself and to earlier positions.

`earlier` is a mask that says which pairs are allowed. Comparing a column
of places with a row of places broadcasts to a `[5, 5]` table of booleans
that is true where the row number is at least the column number: the
diagonal and everything below it. `where` keeps the score where the mask is
true and puts a huge negative number where it is false. `softmax` then
gives those entries a weight of zero.

You can see it in the second table. Everything above the diagonal is zero.
Row 0 can only see itself, so its weight is 1.00. Row 1 splits its weight
between positions 0 and 1, and so on down. Every row adds up to one. This
is called *causal* attention, and the mask is all that makes it so.

The last line takes the weighted averages. `weights` is `[5, 5]`, the
values are `[5, 8]`, and their matrix product is `[5, 8]`: one new vector
for each position, made from the values of the positions it attended to.

## Several heads

One table of weights lets a position look for one kind of thing. Real
transformers run several attentions side by side, called *heads*, each
with its own queries, keys and values, so that one head can track, say,
the last vowel while another tracks where the line began. The standard
trick is to split each vector into equal parts and give one part to each
head: with a width of 64 and 4 heads, every head works with 16 numbers.
The results of the heads are put back side by side and passed through one
more linear layer.

Here is attention as a class, the way we'll use it in the model. It also
handles a batch: `x` now has the shape `[rows, length, width]`.

```aura fragment
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
```

`forward` is Listing 24-3 with two more axes. `split` cuts the last axis
into heads with `reshape` and then moves the heads axis forward with
`permute`, so that the last two axes are `[length, width / heads]`.
`matmul` works on the last two axes and treats any axes before them as a
batch, so one call computes the scores for every row and every head. The
mask has the shape `[length, length]` and broadcasts across both. At the
end, `permute` and `reshape` undo the split.

If the reshapes are hard to follow, print `shape()` after each step with a
small input. The shapes are the best guide to this code, and getting them
wrong gives an `AU4007` error that names the two shapes that did not fit.

The `tensor` module has this layer built in, as `MultiheadAttention`, and
a function `scaled_dot_product_attention` for the middle of it. We wrote
our own because the point of this chapter is to see inside. In the next
section we put it to work.
