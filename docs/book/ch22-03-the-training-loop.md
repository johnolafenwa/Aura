# The training loop

So far each step of training has looked at every example we had. That is
fine for 200 houses. It does not survive contact with a real dataset of a
million rows, and it hides the question we care about most, which is how
the model will do on data it has not seen. This section turns the bare loop
into the one that real training uses, and then looks at the two ways
training goes wrong.

## Batches, epochs, and held-out data

Three changes turn Listing 22-2 into a proper training program.

The first is to hold some data back. We set aside part of the data before
training starts, never train on it, and use it only to measure. The loss on
this *held-out* data, also called validation data, is our estimate of how
the model does on houses it has never seen, and it is the number to watch.

The second is to train on a small *batch* of rows at a time. Each step
computes the loss on, say, 32 houses, and steps on that. The gradient from
32 houses is a noisy estimate of the gradient from all of them, but it is
far cheaper, and many cheap, slightly wrong steps get downhill faster than
a few exact ones. This is the "stochastic" in stochastic gradient descent.

The third is to go through the data in a different random order each time.
One pass over all the training rows is called an *epoch*. If the batches
were the same rows in the same order every epoch, the model could pick up
on the order.

*Filename: src/main.au*

```aura
import random
import tensor
from tensor import Device, Linear, SGD, Tensor

def make_houses(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[float32]):
    # Sizes from 40 to 160 square meters, 1 to 5 bedrooms, ages up to 50 years.
    size = tensor.rand([count, 1], seed, device) * 120.0 + 40.0
    rooms = (tensor.rand([count, 1], seed + 1, device) * 5.0).to_int64().to_float32() + 1.0
    age = tensor.rand([count, 1], seed + 2, device) * 50.0
    noise = tensor.randn([count, 1], seed + 3, device) * 8.0
    price = size * 2.1 + rooms * 14.0 - age * 0.9 + 48.0 + noise
    return (tensor.cat([size, rooms, age], 1), price)

def mean_squared_error(guess: Tensor[float32], truth: Tensor[float32]) -> Tensor[float32]:
    error = guess - truth
    return (error * error).mean()

def main():
    device = Device.cpu()
    all_x, all_y = make_houses(1000, 1, device)
    raw_train = all_x.slice(0, 0, 800)
    train_y = all_y.slice(0, 0, 800)
    raw_held = all_x.slice(0, 800, 1000)
    held_y = all_y.slice(0, 800, 1000)

    mean = raw_train.mean(0)
    centered = raw_train - mean
    spread = (centered * centered).mean(0).sqrt()
    train_x = centered / spread
    held_x = (raw_held - mean) / spread

    mut model = Linear(3, 1, 7, device)
    mut optimizer = SGD(model.parameters(), lr=0.05)
    mut rng = random.Rng(3)
    mut order = list(range(800))
    batch = 32
    for epoch in range(5):
        rng.shuffle(order)
        mut total: float32 = 0.0
        mut at = 0
        while at < 800:
            rows = Tensor[int64].from_array(Array[int64].from_list(order[at:at + batch], [batch]), device)
            guess = model.forward(train_x.index_select(0, rows))
            loss = mean_squared_error(guess, train_y.index_select(0, rows))
            optimizer.zero_grad()
            loss.backward()
            optimizer.step()
            total += loss.item()
            at += batch
        held = mean_squared_error(model.forward(held_x), held_y)
        print(f"epoch {epoch}: training {total / 25.0:.2f} held-out {held.item():.2f}")
```

<span class="caption">Listing 22-6: Shuffled batches, epochs, and a held-out set</span>

```console
$ aura run src/main.au
epoch 0: training 17003.26 held-out 488.05
epoch 1: training 151.19 held-out 59.92
epoch 2: training 64.04 held-out 56.29
epoch 3: training 63.44 held-out 56.34
epoch 4: training 63.60 held-out 56.19
```

We make 1,000 houses and split them with `slice`: the first 800 rows to
train on, the last 200 held out. The rows came out of a random generator in
no particular order, so taking the last fifth is a fair split. With real
data, shuffle before you split, because real files are often sorted by
date or by label.

Look closely at the standardizing. The mean and spread are computed from
the training rows only, and then applied to both sets. It would be easy to
compute them from all 1,000 rows, and it would be a mistake with a name,
*leakage*: information about the held-out rows would reach the model
through the statistics, and the held-out loss would stop being an honest
estimate. The discipline is that nothing about the held-out data touches
training, not even its average.

The shuffling uses the `random` module from
[Chapter 20](/book/ch20-00-library-tour). `order` is a list of the row
numbers 0 to 799, `rng.shuffle(order)` rearranges it at the start of each
epoch, and each batch turns 32 of those row numbers into an integer tensor
for `index_select`, the row-picking operation from Listing 21-10. With 800
rows and batches of 32 there are 25 steps per epoch, and `total / 25.0` is
the average training loss over the epoch.

Now the output. The first epoch's training loss is huge because it averages
over the first steps, when the model knew nothing. By the third epoch both
numbers have settled near the noise floor of 64 that we found in the last
section. The two numbers measure different things and are not expected to
match exactly: the training figure is an average taken while the model was
still changing, and the held-out figure is from 200 different houses.
What matters is that the held-out loss is about as good as the training
loss. The model has learned something true about houses in general.

## When a model memorizes

The linear model for houses could not go far wrong, because the truth
really was a weighted sum. Let's look at a case where the model has more
freedom than the data can pin down.

We'll take 12 measurements of a quantity that follows a smooth wave, with a
little noise in each measurement, and fit a curve through them. The model
is still a `Linear` layer, with a twist in its input: for each point `x` we
feed it `x`, `x` squared, `x` cubed, and so on up to some power, which lets
a weighted sum draw a curve with that many bends. The highest power is
called the *degree*. Degree 1 is a straight line. The more powers we give
the model, the more shapes it can make.

```aura
import tensor
from tensor import AdamW, Device, Linear, Tensor

def curve(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[float32]):
    # Points on a wave, measured with some noise.
    x = tensor.rand([count, 1], seed, device) * 6.0 - 3.0
    y = x.sin() + tensor.randn([count, 1], seed + 1, device) * 0.2
    return (x, y)

def powers(x: Tensor[float32], degree: int64) -> Tensor[float32]:
    # Columns x, x^2, ..., x^degree, with x scaled into -1 to 1.
    scaled = x / 3.0
    mut columns: list[Tensor[float32]] = []
    mut power = scaled.clone()
    for at in range(degree):
        columns.append(power.clone())
        power = power * scaled
    return tensor.cat(columns, 1)

def fit(degree: int64, device: Device) -> None:
    x, y = curve(12, 5, device)
    held_x, held_y = curve(200, 50, device)
    mut model = Linear(degree, 1, 1, device)
    mut optimizer = AdamW(model.parameters(), lr=0.01, weight_decay=0.0)
    for step in range(3001):
        error = model.forward(powers(x, degree)) - y
        loss = (error * error).mean()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 500 == 0:
            held = model.forward(powers(held_x, degree)) - held_y
            print(f"degree {degree} step {step}: training {loss.item():.4f} held-out {(held * held).mean().item():.4f}")

def main():
    device = Device.cpu()
    fit(1, device)
    fit(3, device)
    fit(11, device)
```

<span class="caption">Listing 22-7: Three models of increasing freedom on 12 points</span>

```console
$ aura run src/main.au
degree 1 step 0: training 1.8589 held-out 1.4425
degree 1 step 500: training 0.1715 held-out 0.2214
degree 1 step 1000: training 0.1715 held-out 0.2214
degree 1 step 1500: training 0.1715 held-out 0.2214
degree 1 step 2000: training 0.1715 held-out 0.2214
degree 1 step 2500: training 0.1715 held-out 0.2214
degree 1 step 3000: training 0.1715 held-out 0.2214
degree 3 step 0: training 1.2438 held-out 0.9409
degree 3 step 500: training 0.0581 held-out 0.1082
degree 3 step 1000: training 0.0133 held-out 0.0590
degree 3 step 1500: training 0.0116 held-out 0.0557
degree 3 step 2000: training 0.0116 held-out 0.0555
degree 3 step 2500: training 0.0116 held-out 0.0555
degree 3 step 3000: training 0.0116 held-out 0.0555
degree 11 step 0: training 0.8047 held-out 0.6309
degree 11 step 500: training 0.0397 held-out 0.1070
degree 11 step 1000: training 0.0192 held-out 0.0623
degree 11 step 1500: training 0.0120 held-out 0.0555
degree 11 step 2000: training 0.0079 held-out 0.0634
degree 11 step 2500: training 0.0050 held-out 0.0776
degree 11 step 3000: training 0.0033 held-out 0.0944
```

This one takes about twenty seconds, because it trains three models for
3,000 steps each. The optimizer is `AdamW`, which we'll come to below. Read the
three blocks of output as three stories.

The straight line, degree 1, stops improving almost at once, with a
training loss of 0.17 and a held-out loss of 0.22. Both are poor. A line
can't follow a wave, and no amount of training will change that. This is
*underfitting*: the model is too simple for the pattern.

Degree 3 reaches a training loss of 0.012 and a held-out loss of 0.056. The
noise we added has a typical size of 0.2, so the floor is 0.2 squared,
0.04. This model is close to as good as the data allows.

Degree 11 is the interesting one. Its training loss goes lower than
anything else on the page and is still falling at the end. Its held-out
loss falls to about the level of the degree 3 model, and then turns around
and climbs, step after step, while the training loss keeps improving. With
twelve points and eleven powers to play with, the model has enough freedom
to bend its curve through every training point, noise and all. It has
stopped learning the wave and started memorizing the twelve measurements.
Between the points its curve swings wildly, and the held-out data, which
lies between the points, shows it. This is *overfitting*, and it is the
central hazard of machine learning.

The training loss could not have told us. By its account the degree 11
model is the best of the three. Only the held-out loss shows the truth,
which is why we never judge a model by the data it trained on.

## What to do about overfitting

The pattern in the degree 11 output suggests the first remedy. The held-out
loss was at its best around step 1,500. If we had stopped there, we would
have had a good model. *Early stopping* means exactly that: watch the
held-out loss, and keep the weights from the point where it was lowest. It
costs nothing and it is used everywhere.

The second remedy is more data. Overfitting is a mismatch between how
flexible the model is and how much the data constrains it. Change `12` to
`200` in `fit` and the degree 11 model can no longer thread every point, so
it settles for the wave: its held-out loss falls to 0.046 and is still
improving at the last step.

The third is to make the model prefer simple answers. A curve that swings
wildly between points needs large weights, positive and negative, that
nearly cancel. So we add a gentle pull on every weight toward zero, which
the model can resist only where the data strongly insists. This is called
*weight decay*, and `AdamW` has it built in:

```aura
import tensor
from tensor import AdamW, Device, Linear, Tensor

def curve(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[float32]):
    # Points on a wave, measured with some noise.
    x = tensor.rand([count, 1], seed, device) * 6.0 - 3.0
    y = x.sin() + tensor.randn([count, 1], seed + 1, device) * 0.2
    return (x, y)

def powers(x: Tensor[float32], degree: int64) -> Tensor[float32]:
    # Columns x, x^2, ..., x^degree, with x scaled into -1 to 1.
    scaled = x / 3.0
    mut columns: list[Tensor[float32]] = []
    mut power = scaled.clone()
    for at in range(degree):
        columns.append(power.clone())
        power = power * scaled
    return tensor.cat(columns, 1)

def fit(decay: float64, device: Device) -> None:
    x, y = curve(12, 5, device)
    held_x, held_y = curve(200, 50, device)
    mut model = Linear(11, 1, 1, device)
    mut optimizer = AdamW(model.parameters(), lr=0.01, weight_decay=decay)
    mut last: float32 = 0.0
    for step in range(3001):
        error = model.forward(powers(x, 11)) - y
        loss = (error * error).mean()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        last = loss.item()
    held = model.forward(powers(held_x, 11)) - held_y
    print(f"decay {decay}: training {last:.4f} held-out {(held * held).mean().item():.4f}")

def main():
    device = Device.cpu()
    fit(0.0, device)
    fit(0.1, device)
    fit(2.0, device)
```

<span class="caption">Listing 22-8: The degree 11 model with three strengths of weight decay</span>

```console
$ aura run src/main.au
decay 0.0: training 0.0033 held-out 0.0944
decay 0.1: training 0.0106 held-out 0.0601
decay 2.0: training 0.2445 held-out 0.2729
```

With no decay we get the overfit model from before. A decay of 0.1 gives a
worse training loss and a clearly better held-out loss: we have traded a
little fit on the twelve points for a curve that behaves between them. A
decay of 2.0 goes too far. The pull toward zero is now so strong that the
model can't follow the wave at all, and both losses are bad. We have pushed
an overfitting model into underfitting.

That trade, between a model too rigid to learn the pattern and one free
enough to learn the noise, runs through everything that follows. Settings
like the learning rate, the weight decay, the degree, and the number of
steps are called *hyperparameters*: they are not learned by gradient
descent, so we choose them, by trying values and comparing held-out losses.
The held-out data is what makes that comparison honest.

## A better optimizer

`SGD` moves every weight by the same learning rate times its slope. That
works when the slopes are of similar size, which is why we have been
standardizing our inputs. `AdamW` is the optimizer most models are trained
with today. It keeps a running average of each weight's recent gradients
and of their typical size, and gives every weight a step scaled to its own
history, so a weight whose slopes are consistently tiny still makes
progress.

The clearest way to see the difference is to make life hard for both.
Here are the houses again, this time without standardizing, so that the
size column is in the hundreds while the bedroom column is in single
digits:

```aura
import tensor
from tensor import AdamW, Device, Linear, SGD, Tensor

def make_houses(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[float32]):
    # Sizes from 40 to 160 square meters, 1 to 5 bedrooms, ages up to 50 years.
    size = tensor.rand([count, 1], seed, device) * 120.0 + 40.0
    rooms = (tensor.rand([count, 1], seed + 1, device) * 5.0).to_int64().to_float32() + 1.0
    age = tensor.rand([count, 1], seed + 2, device) * 50.0
    noise = tensor.randn([count, 1], seed + 3, device) * 8.0
    price = size * 2.1 + rooms * 14.0 - age * 0.9 + 48.0 + noise
    return (tensor.cat([size, rooms, age], 1), price)

def mean_squared_error(guess: Tensor[float32], truth: Tensor[float32]) -> Tensor[float32]:
    error = guess - truth
    return (error * error).mean()

def main():
    device = Device.cpu()
    x, y = make_houses(800, 1, device)
    mut plain = Linear(3, 1, 7, device)
    mut adaptive = Linear(3, 1, 7, device)
    mut sgd = SGD(plain.parameters(), lr=0.00005)
    mut adam = AdamW(adaptive.parameters(), lr=0.05, weight_decay=0.0)
    for step in range(3001):
        plain_loss = mean_squared_error(plain.forward(x), y)
        plain_loss.backward()
        sgd.step()
        adaptive_loss = mean_squared_error(adaptive.forward(x), y)
        adaptive_loss.backward()
        adam.step()
        if step % 1000 == 0:
            print(f"step {step}: SGD {plain_loss.item():.1f} AdamW {adaptive_loss.item():.1f}")
```

<span class="caption">Listing 22-9: `SGD` and `AdamW` on columns of very different scales</span>

```console
$ aura run src/main.au
step 0: SGD 67984.0 AdamW 67984.0
step 1000: SGD 676.4 AdamW 238.9
step 2000: SGD 474.9 AdamW 130.7
step 3000: SGD 356.4 AdamW 99.7
```

Both models start from the same seed, so they begin identical. The learning
rate for `SGD` is tiny because it has to be. The size column produces such
large slopes that doubling the rate to 0.0001 makes the loss explode, and at
a rate small enough for that column, the bias and the bedroom weight barely
move. `AdamW` takes a learning rate a thousand times larger and is
well ahead throughout.

Neither is anywhere near the floor of 64, though, and Listing 22-6 got
there in three epochs. A better optimizer softens the cost of badly scaled
data and does not remove it. Standardize your inputs, and then use `AdamW`
with a learning rate around 0.001 as the default starting point for
anything larger than the models in this chapter. `AdamW(params)` with no
other arguments uses exactly that rate, with a weight decay of 0.01.

This loop also leaves out `zero_grad()`, to show that it runs correctly
without it: `step()` cleared each model's gradients.

## Summary

A training program is a model, a loss, an optimizer, and a loop of four
lines: forward, `zero_grad()`, `backward()`, `step()`. A `Parameter` is a
tensor that training may replace, a layer such as `Linear` owns parameters,
and `parameters()` hands them to an optimizer, which the compiler allows
only for a model declared `mut`. Mean squared error is the loss for
predicting numbers, and cross entropy on raw scores is the loss for
predicting classes.

Around that loop sit the habits that make its results trustworthy. Hold
data out and judge the model by it. Compute every statistic from the
training rows alone. Train in shuffled batches. Watch for a held-out loss
that turns upward while the training loss keeps falling, and answer it by
stopping early, adding data, or adding weight decay.

Every model so far has been a single `Linear` layer, and a single layer can
only draw straight boundaries. The curve fit needed us to supply the powers
of `x` by hand. In the next chapter the model learns its own bends: we
stack layers into a neural network, give the network a class of its own,
save it to a file, and teach one to see.
