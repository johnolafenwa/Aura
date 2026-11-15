# Classification

A botanist hands us measurements of 150 flowers: the length and width of a
petal, in centimeters, for 50 flowers of each of three species. We want a
program that takes the two measurements of a new flower and says which
species it is. The answer is not a number on a scale. It is one of three
names, and that changes what the model outputs and how we score it.

We'll manufacture the flowers, as we did the houses, so that the listings
are self-contained. Each species gets a typical petal length and width, and
each flower is that typical flower plus some random variation.

## Scores and probabilities

A model can't output a name. What it can output is one number per species,
a *score*, with the understanding that the highest score is its answer. So
the model for this problem takes rows of 2 measurements and gives rows of 3
scores, and it can be the same `Linear` layer we used for prices, with three
output columns in place of one:

*Filename: src/main.au*

```aura
import tensor
from tensor import Device, Linear, Tensor

def blob(count: int64, length: float64, width: float64, seed: int64, device: Device) -> Tensor[float32]:
    lengths = tensor.randn([count, 1], seed, device) * 0.5 + length
    widths = tensor.randn([count, 1], seed + 1, device) * 0.25 + width
    return tensor.cat([lengths, widths], 1)

def flowers(each: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Petal length and width, in centimeters, of three species.
    x = tensor.cat([
        blob(each, 1.5, 0.3, seed, device),
        blob(each, 4.3, 1.3, seed + 2, device),
        blob(each, 5.6, 2.0, seed + 4, device),
    ], 0)
    labels = tensor.cat([
        Tensor[int64].full([each], 0, device),
        Tensor[int64].full([each], 1, device),
        Tensor[int64].full([each], 2, device),
    ], 0)
    return (x, labels)

def main():
    device = Device.cpu()
    x, labels = flowers(50, 10, device)
    print(x.shape())
    print(labels.shape())
    model = Linear(2, 3, 3, device)
    scores = model.forward(x)
    print(scores.slice(0, 0, 2).to_array())
    chances = scores.softmax()
    print(chances.slice(0, 0, 2).to_array())
    print(chances.slice(0, 0, 2).sum(1).to_array())
    print(scores.argmax().slice(0, 0, 10).to_array())
```

<span class="caption">Listing 22-3: Three scores per flower from an untrained model</span>

```console
$ aura run src/main.au
[150, 2]
[150]
Array[float32](shape=[2, 3], values=[0.35865748, -0.19352716, 0.38584343, 0.015208602, 0.16653308, -0.26836824])
Array[float32](shape=[2, 3], values=[0.3841353, 0.22114313, 0.39472163, 0.3428815, 0.3988995, 0.25821903])
Array[float32](shape=[2], values=[1.0, 1.0])
Array[int64](shape=[10], values=[2, 1, 0, 0, 0, 0, 0, 0, 0, 1])
```

The data comes as two tensors. `x` is `[150, 2]`, the measurements. `labels`
is `[150]`, and it is a `Tensor[int64]`: the species of each flower as 0, 1,
or 2. The numbers are names, not amounts. Species 2 is not twice species 1,
which is exactly why we don't ask the model to output the label as a single
number. `tensor.cat(tensors, 0)` stacks the three groups of 50 one after
another along axis 0, so the first 50 rows are species 0.

The model is untrained, so its scores, printed for the first two flowers,
are meaningless small numbers. People call raw scores like these *logits*.
They can be any size and either sign, which makes them awkward to
interpret, so we usually pass them through `softmax()`. Softmax turns each
row of scores into positive numbers that add up to one, keeping their
order: the highest score gets the largest share. We can read a row of
softmax output as the model's probabilities for the three species. Right
now the first flower gets 38%, 22%, and 39%, which is a shrug.

`scores.argmax()` picks the position of the highest score in each row, and
is the model's actual answer. The first ten flowers are all species 0, and
before training the model's picks for them are a mix of all three. This
model is declared without `mut`, by the way: we only looked at it.

## Cross entropy

For prices, the loss was the squared distance between guess and truth. For
classes there is no distance. There is the probability the model gave to the
right answer, and we want a loss that is small when that probability is
near one and large when it is near zero.

The standard choice is the negative logarithm of that probability. If the
model gives the correct species a probability of 1, the loss for that flower
is zero. At a probability of one half the loss is 0.69, at one tenth it is
2.3, and as the probability approaches zero the loss grows without limit,
so a confident wrong answer is punished far harder than an unsure one. The
average of this over all the flowers is called the *cross entropy*.

Let's compute it by hand once, to see that there is nothing more to it:

```aura
import tensor
from tensor import Device, Linear, Tensor

def blob(count: int64, length: float64, width: float64, seed: int64, device: Device) -> Tensor[float32]:
    lengths = tensor.randn([count, 1], seed, device) * 0.5 + length
    widths = tensor.randn([count, 1], seed + 1, device) * 0.25 + width
    return tensor.cat([lengths, widths], 1)

def flowers(each: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Petal length and width, in centimeters, of three species.
    x = tensor.cat([
        blob(each, 1.5, 0.3, seed, device),
        blob(each, 4.3, 1.3, seed + 2, device),
        blob(each, 5.6, 2.0, seed + 4, device),
    ], 0)
    labels = tensor.cat([
        Tensor[int64].full([each], 0, device),
        Tensor[int64].full([each], 1, device),
        Tensor[int64].full([each], 2, device),
    ], 0)
    return (x, labels)

def main():
    device = Device.cpu()
    x, labels = flowers(50, 10, device)
    model = Linear(2, 3, 3, device)
    scores = model.forward(x)

    correct = tensor.one_hot(labels, 3)
    print(correct.slice(0, 0, 2).to_array())
    log_chances = scores.log_softmax()
    picked = tensor.where(correct, log_chances, scores.scalar(0.0))
    by_hand = -picked.sum() / 150.0
    print(by_hand.item())

    print(tensor.cross_entropy(scores, labels).item())
```

<span class="caption">Listing 22-4: Cross entropy, by hand and from the module</span>

```console
$ aura run src/main.au
Array[bool](shape=[2, 3], values=[true, false, false, true, false, false])
1.8331043
1.8331043
```

`tensor.one_hot(labels, 3)` turns the labels into a mask with one row per
flower and one column per species, true in the column the label names. The
first two flowers are species 0, so their rows are true in column 0.
`log_softmax()` is the logarithm of the softmax, computed in one step
because taking the two separately loses precision when a probability is
tiny. `where` keeps the log probability of the correct species in each row
and zero elsewhere, and summing, negating, and dividing by the number of
flowers gives the average.

`tensor.cross_entropy(scores, labels)` does all of that and prints the same
number. It takes the raw scores, not the softmax output, and it takes the
labels as a `Tensor[int64]` of class numbers. Passing probabilities where
scores belong is a common mistake, and it does not fail: the model trains,
badly.

Is 1.83 a good loss? There is a useful number to compare against. A model
that knew nothing and gave every species a third would score the negative
logarithm of one third, which is 1.10. Our untrained model is worse than
knowing nothing, because its random weights happen to favor the wrong
species for most of the flowers.

## Training a classifier

The loop is the one from the last section with a different loss. We'll also
report *accuracy*, the fraction of flowers the model gets right, because a
loss of 0.28 means little to a botanist and "96% correct" means a lot:

```aura
import tensor
from tensor import Device, Linear, SGD, Tensor

def blob(count: int64, length: float64, width: float64, seed: int64, device: Device) -> Tensor[float32]:
    lengths = tensor.randn([count, 1], seed, device) * 0.5 + length
    widths = tensor.randn([count, 1], seed + 1, device) * 0.25 + width
    return tensor.cat([lengths, widths], 1)

def flowers(each: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Petal length and width, in centimeters, of three species.
    x = tensor.cat([
        blob(each, 1.5, 0.3, seed, device),
        blob(each, 4.3, 1.3, seed + 2, device),
        blob(each, 5.6, 2.0, seed + 4, device),
    ], 0)
    labels = tensor.cat([
        Tensor[int64].full([each], 0, device),
        Tensor[int64].full([each], 1, device),
        Tensor[int64].full([each], 2, device),
    ], 0)
    return (x, labels)

def accuracy(scores: Tensor[float32], labels: Tensor[int64]) -> float32:
    right = scores.argmax().equal(labels)
    return tensor.where(right, scores.scalar(1.0), scores.scalar(0.0)).mean().item()

def main():
    device = Device.cpu()
    x, labels = flowers(50, 10, device)
    mut model = Linear(2, 3, 3, device)
    mut optimizer = SGD(model.parameters(), lr=0.1)
    for step in range(1001):
        scores = model.forward(x)
        loss = tensor.cross_entropy(scores, labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 250 == 0:
            print(f"step {step}: loss {loss.item():.3f} accuracy {accuracy(scores, labels):.3f}")

    new_x, new_labels = flowers(20, 99, device)
    print(f"new flowers: accuracy {accuracy(model.forward(new_x), new_labels):.3f}")

    one = Tensor[float32].from_array(Array[float32].from_list([4.9, 1.6], [1, 2]), device)
    print(model.forward(one).softmax().to_array())
```

<span class="caption">Listing 22-5: A classifier for three species</span>

```console
$ aura run src/main.au
step 0: loss 1.833 accuracy 0.233
step 250: loss 0.488 accuracy 0.940
step 500: loss 0.369 accuracy 0.960
step 750: loss 0.315 accuracy 0.960
step 1000: loss 0.282 accuracy 0.960
new flowers: accuracy 0.933
Array[float32](shape=[1, 3], values=[0.00856133, 0.46752018, 0.52391845])
```

`accuracy` compares the model's pick for each flower with its label, which
gives a mask, and turns the mask into ones and zeros so that the mean is the
fraction that matched. Aura does not convert a `bool` tensor to floats for
us, so `where` does it.

Accuracy climbs from 23% to 96%. The loss keeps falling after the accuracy
has stopped improving, and that is normal: the model is growing more
confident about flowers it already classifies correctly. This is also why
we train on the loss and not on accuracy. Accuracy moves in jumps, when a
flower flips from wrong to right, and has no slope in between, so it gives
gradient descent nothing to follow. The loss changes smoothly with every
weight.

The line after the loop is the test that matters. `flowers(20, 99, device)`
makes 60 flowers the model has never seen, from a different seed, and it
gets 93% of them right. Doing well on the training data proves little. A
model could score perfectly there by memorizing. Doing well on new data is
the goal, and the next section is about measuring it honestly.

The last line classifies one flower with a petal 4.9 centimeters long and
1.6 wide. The model answers 1% species 0, 47% species 1, 52% species 2.
Those measurements sit between the typical flowers of species 1 and 2, and
the model's near even split says so. A classifier that reports
probabilities can tell us when it is unsure, which is often as useful as
the answer.

Why does the model top out at 96% on its own training data? Because species
1 and 2 overlap. Some flowers of species 1 really are larger than some of
species 2, and no rule based on these two measurements can separate them.
Like the noise floor in the house prices, that is a property of the data.
