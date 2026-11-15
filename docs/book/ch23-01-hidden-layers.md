# Hidden layers

Here is a problem that looks easy. We scatter points over a square, and
label each point 1 if it lies inside a circle in the middle and 0 if it lies
outside. Given a new point, say which side it is on. A person looking at a
plot would see the circle at once. Let's see what the classifier from
Chapter 22 makes of it.

## Where a single layer fails

*Filename: src/main.au*

```aura
import tensor
from tensor import AdamW, Device, Linear, Tensor

def ring(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Points in a square, labeled 1 inside the circle of radius 1 and 0 outside.
    x = tensor.rand([count, 2], seed, device) * 3.0 - 1.5
    inside = (x * x).sum(1).less(Tensor[float32].full([1], 1.0, device))
    labels = tensor.where(inside, Tensor[int64].full([1], 1, device), Tensor[int64].full([1], 0, device))
    return (x, labels)

def accuracy(scores: Tensor[float32], labels: Tensor[int64]) -> float32:
    right = scores.argmax().equal(labels)
    return tensor.where(right, scores.scalar(1.0), scores.scalar(0.0)).mean().item()

def main():
    device = Device.cpu()
    x, labels = ring(400, 1, device)
    held_x, held_labels = ring(400, 2, device)
    print(f"{labels.sum().item()} of 400 points are inside")

    mut model = Linear(2, 2, 3, device)
    mut optimizer = AdamW(model.parameters(), lr=0.01)
    for step in range(501):
        loss = tensor.cross_entropy(model.forward(x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    print(f"held-out accuracy {accuracy(model.forward(held_x), held_labels):.3f}")
```

<span class="caption">Listing 23-1: A linear classifier on points inside and outside a circle</span>

```console
$ aura run src/main.au
146 of 400 points are inside
held-out accuracy 0.655
```

`ring` makes points with both coordinates between -1.5 and 1.5. A point is
inside the circle when the sum of its squared coordinates is less than one,
which is one comparison on the whole table, and `where` turns the resulting
mask into labels of 1 and 0.

The model scores 65.5% on held-out points, and that number is worse than it
looks. 146 of the 400 training points are inside the circle, so 63.5% are
outside, and a program that answered "outside" for every point without
looking would score about that. Our model has learned almost nothing.

It can't do better, and more training won't help. A linear layer computes a
weighted sum of the two coordinates, and the places where two such scores
are equal form a straight line. So a linear classifier divides the square
with a straight line and calls one side "inside". No straight line
separates the inside of a circle from the outside. The model is too simple
for the pattern, which is the underfitting we met in the last chapter.

## Stacking layers, and why that alone is not enough

The natural idea is to use two layers: feed the output of one `Linear` into
another. On its own, that buys nothing. A weighted sum of weighted sums is
still a weighted sum, so two linear layers in a row can compute exactly
what one can, and the boundary is still a straight line.

What breaks the pattern is a small bend between the layers, called an
*activation*. The most common one is `relu`, which replaces every negative
number with zero and leaves the rest alone. It looks too simple to matter.
Here is what it does: the first layer computes, say, 16 different weighted
sums of the coordinates. Each one is positive on one side of some line
across the square and negative on the other. After `relu`, each is zero on
one side of its line and rises steadily on the other, like a ramp with a
fold in it. The second layer adds the 16 folded ramps together, and a sum
of folds can approximate any shape we like, the way a circle can be
approximated by a polygon. Training chooses where the folds go.

The layer between the input and the output is called a *hidden layer*,
because its values are neither given to us nor asked of us. Its 16 outputs
are the features the network invents for itself, the job we did by hand
when we computed powers of `x`.

```aura
import tensor
from tensor import AdamW, Device, Linear, Tensor

def ring(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Points in a square, labeled 1 inside the circle of radius 1 and 0 outside.
    x = tensor.rand([count, 2], seed, device) * 3.0 - 1.5
    inside = (x * x).sum(1).less(Tensor[float32].full([1], 1.0, device))
    labels = tensor.where(inside, Tensor[int64].full([1], 1, device), Tensor[int64].full([1], 0, device))
    return (x, labels)

def accuracy(scores: Tensor[float32], labels: Tensor[int64]) -> float32:
    right = scores.argmax().equal(labels)
    return tensor.where(right, scores.scalar(1.0), scores.scalar(0.0)).mean().item()

def predict(hidden: Linear, output: Linear, x: Tensor[float32]) -> Tensor[float32]:
    return output.forward(hidden.forward(x).relu())

def draw(hidden: Linear, output: Linear, device: Device) -> None:
    # The model's answer at each point of a grid over the square.
    for row in range(13):
        mut points: list[float32] = []
        for column in range(31):
            points.append((column.to_float() * 0.1 - 1.5) as float32)
            points.append((1.5 - row.to_float() * 0.25) as float32)
        grid = Tensor[float32].from_array(Array[float32].from_list(points, [31, 2]), device)
        picks = predict(hidden, output, grid).argmax().to_array()
        mut line = ""
        for column in range(31):
            if picks[column] == 1:
                line += "#"
            else:
                line += "."
        print(line)

def main():
    device = Device.cpu()
    x, labels = ring(400, 1, device)
    held_x, held_labels = ring(400, 2, device)

    mut hidden = Linear(2, 16, 3, device)
    mut output = Linear(16, 2, 5, device)
    mut params = hidden.parameters()
    params.extend(output.parameters())
    mut optimizer = AdamW(params, lr=0.01)
    for step in range(1001):
        loss = tensor.cross_entropy(predict(hidden, output, x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 250 == 0:
            held = accuracy(predict(hidden, output, held_x), held_labels)
            print(f"step {step}: loss {loss.item():.3f} held-out accuracy {held:.3f}")
    draw(hidden, output, device)
```

<span class="caption">Listing 23-2: One hidden layer of 16 units learns the circle</span>

```console
$ aura run src/main.au
step 0: loss 0.785 held-out accuracy 0.493
step 250: loss 0.066 held-out accuracy 0.993
step 500: loss 0.036 held-out accuracy 0.993
step 750: loss 0.025 held-out accuracy 0.993
step 1000: loss 0.019 held-out accuracy 0.993
...............................
...............................
...............................
.........#############.........
......##################.......
......###################......
.....####################......
......###################......
.......##################......
.........#############.........
...............................
...............................
...............................
```

The whole model is the one line in `predict`: a layer from 2 inputs to 16,
`relu`, and a layer from 16 to 2 scores. Held-out accuracy goes from a coin
flip to 99.3%.

`draw` shows what was learned. It asks the trained model about every point
of a grid, 31 columns by 13 rows covering the square, and prints `#` where
the model answers "inside". Nobody gave the program a circle, a radius, or
the formula for distance. It was given 400 labeled points and a model
flexible enough to fold a boundary around them, and it found the circle. If
you look closely the outline is not quite round. It is a polygon, made from
the 16 folds, and its corners are where the few mistakes are.

The training loop did not change. `loss.backward()` computes gradients for
both layers at once, walking back from the loss through the second layer,
through the `relu`, and through the first. That is the reason for building
everything on gradients in the first place: the first layer has no target
of its own to aim for, and it does not need one. It is adjusted by how much
each of its weights contributes to the final loss.

There are other activations. `tanh()` and `sigmoid()` are smooth S-shaped
curves that were standard in older networks, and `gelu()` is a smooth
relative of `relu` that transformers use; we'll meet it in the next
chapter. For most networks `relu` is the place to start.

## The seam that is showing

One part of Listing 23-2 is awkward. An optimizer wants one list of
parameters, and we have two layers, so we asked each for its parameters and
joined the lists by hand. The model itself isn't a thing at all: it is two
bindings and a function that happens to use both, and every helper has to
take both layers as arguments. A network with a dozen layers would be
unpleasant to write this way, and easy to get wrong. Leave one layer out of
the list and it silently never trains.

A model should be a single value that owns its layers. Aura has classes
for exactly that, and the `tensor` module has a trait that makes a class
into a model.
