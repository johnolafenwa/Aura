# Gradients

In the last section we guessed a price for every house from four numbers
that we made up. Learning means finding those numbers from the data. To do
that a program needs two things: a way to measure how wrong the current
numbers are, and a way to know which direction to move each of them to be
less wrong. The first is a formula we choose. The second is a *gradient*,
and computing gradients is the one thing the `tensor` module does that you
could not reasonably do by hand.

## Measuring how wrong we are

Here are the prices the six houses sold for, in thousands: 137, 225, 334,
168, 410, and 262. Given a guess for each, the usual measure of how wrong the
guesses are is the *mean squared error*: take each difference between guess
and truth, square it so that too high and too low both count as positive,
and average. One number comes out, called the *loss*. A loss of zero means
every guess was exact, and larger is worse. Training is the search for the
numbers that make the loss as small as it will go.

Squaring has a second effect that we want. A guess that is off by 20 costs
four times what a guess off by 10 costs, so the search will work hardest on
its worst mistakes.

## The slope of one number

Before the whole model, take the smallest possible case: a function of one
number. Let `y` be `x * x + 2 * x`. At `x = 3` that is 15. If we nudge `x`
up to 3.001, `y` becomes 15.008001. So near 3, each unit of change in `x`
produces about 8 units of change in `y`. That ratio is the *slope* of `y`
with respect to `x`, and it is the piece of information learning runs on: if
`y` were a loss, the slope of 8 says that increasing `x` makes things worse,
so we should decrease it, and it says how strongly.

We could find every slope by nudging, but a real model has millions of
numbers, and nudging each in turn means running the model millions of times
for one step of learning. Aura finds all the slopes at once:

*Filename: src/main.au*

```aura
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    x = Tensor[float32].full([1], 3.0, device).requires_grad()
    y = x * x + x * 2.0
    print(y.item())
    print(y.gradient(x).item())
```

<span class="caption">Listing 21-11: The slope of `y` at `x = 3`</span>

```console
$ aura run src/main.au
15.0
8.0
```

`requires_grad()` gives back the tensor marked as one we intend to ask
about. From then on, every float tensor computed from it remembers how it
was made: `y` knows it is the sum of a product of `x` with itself and a
product of `x` with 2. `y.gradient(x)` walks that record backwards, from `y`
to `x`, applying the rule for each operation on the way, and arrives at the
exact slope. Walking backwards is why the technique is called
*backpropagation*. There is nothing approximate about it. It is the chain
rule from a calculus course, applied by a program that does not get bored.

The record costs a little memory, which is why we have to ask for it. A
tensor that was never marked can't be asked about:

```aura
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    x = Tensor[float32].full([1], 3.0, device)
    y = x * x + x * 2.0
    print(y.gradient(x).item())
```

```console
$ aura run src/main.au
error[AU4007]: `tensor.gradient` expects a tensor made with `requires_grad()` or held by a `Parameter`
 --> src/main.au
  = note: Aura call chain (innermost first): tensor::Tensor.gradient at 216:12 -> main at 3:1
```

## A slope for every weight

A *gradient* is the same idea for a tensor of many numbers: one slope for
each element, in the same shape. Let's ask for the gradient of the loss on
our houses with respect to the weights. We'll use the standardized table
from Listing 21-7, and start with every weight at zero, which guesses a
price of zero for every house:

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def standardize(x: Tensor[float32]) -> Tensor[float32]:
    centered = x - x.mean(0)
    return centered / (centered * centered).mean(0).sqrt()

def main():
    device = Device.cpu()
    x = standardize(houses(device))
    prices = Tensor[float32].from_array(
        Array[float32].from_list([137.0, 225.0, 334.0, 168.0, 410.0, 262.0], [6, 1]),
        device,
    )
    weights = Tensor[float32].zeros([3, 1], device).requires_grad()
    base = Tensor[float32].zeros([1], device).requires_grad()
    error = x.matmul(weights) + base - prices
    loss = (error * error).mean()
    print(f"loss {loss.item()}")
    print(loss.gradient(weights).to_array())
    print(loss.gradient(base).to_array())
```

<span class="caption">Listing 21-12: The loss of an all-zero model, and its gradient</span>

```console
$ aura run src/main.au
loss 74319.664
Array[float32](shape=[3, 1], values=[-187.16936, -177.90735, 164.09938])
Array[float32](shape=[1], values=[-512.0])
```

The loss is enormous, as it should be for a model that prices every house at
nothing. The gradient is the useful part. It has the shape of `weights`, one
slope per weight, and each sign tells us something. The slopes for size and
bedrooms are negative: increasing those weights would lower the loss, so
bigger houses with more bedrooms should be priced higher. The slope for age
is positive: that weight should go down, so older houses should be priced
lower. Nobody told the program any of that. It fell out of the data and the
arithmetic. The slope for `base` is the largest, because the fastest way to
improve a model that guesses zero for everything is to raise all its guesses
at once.

Only `x` and `prices` were not marked, because they are the data and we have
no intention of changing them.

## Walking downhill

A gradient points uphill, toward a larger loss. So we take a small step the
other way: subtract a fraction of each slope from its weight. Then we
measure again from the new position and repeat. This is *gradient descent*,
and with one refinement or another it is how every model in this book, and
every large model you have heard of, was trained.

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def standardize(x: Tensor[float32]) -> Tensor[float32]:
    centered = x - x.mean(0)
    return centered / (centered * centered).mean(0).sqrt()

def main():
    device = Device.cpu()
    x = standardize(houses(device))
    prices = Tensor[float32].from_array(
        Array[float32].from_list([137.0, 225.0, 334.0, 168.0, 410.0, 262.0], [6, 1]),
        device,
    )
    mut weights = Tensor[float32].zeros([3, 1], device).requires_grad()
    mut base = Tensor[float32].zeros([1], device).requires_grad()
    for step in range(301):
        error = x.matmul(weights) + base - prices
        loss = (error * error).mean()
        if step % 50 == 0:
            print(f"step {step}: loss {loss.item()}")
        slope_w = loss.gradient(weights)
        slope_b = loss.gradient(base)
        weights = (weights - slope_w * 0.1).detach().requires_grad()
        base = (base - slope_b * 0.1).detach().requires_grad()
    print(weights.to_array())
    print(base.item())
    print((x.matmul(weights) + base).to_array())
```

<span class="caption">Listing 21-13: Gradient descent, written out by hand</span>

```console
$ aura run src/main.au
step 0: loss 74319.664
step 50: loss 40.92534
step 100: loss 29.648096
step 150: loss 21.616379
step 200: loss 15.878156
step 250: loss 11.778378
step 300: loss 8.849277
Array[float32](shape=[3, 1], values=[64.07492, 16.317204, -16.017256])
255.99998
Array[float32](shape=[6, 1], values=[133.81377, 229.38875, 330.96512, 167.57295, 408.73773, 265.52158])
```

In fifty steps the loss falls from about 74,000 to about 41, and it keeps
shrinking. The last line prints the model's price for each house, and they
are within a few thousand of what the houses sold for: 133.8 against 137,
229.4 against 225. The program has learned to price houses, from six
examples, without being told the rule.

The fraction we multiply the slopes by, `0.1` here, is the *learning rate*.
It is the most important setting in any training program. Too small, and
the loss creeps down so slowly that training never finishes. Too large, and
each step overshoots the bottom of the valley and lands higher up the far
side, so the loss grows without bound. Change `0.1` to `1.5` in Listing
21-13 and you can watch that happen: by step 50 the loss has overflowed and
prints as `NaN`, "not a number". There is no formula for the right
value. People try a few, a factor of three or ten apart, and keep the
largest that still goes down smoothly.

The two lines that take the step need a word. `weights - slope_w * 0.1` is a
new tensor, and because it was computed from `weights` it carries the record
of this step's whole calculation. We don't want that: the next step should
start fresh from the new numbers. `detach()` gives the same numbers as a
constant with no history, and `requires_grad()` marks that constant as the
thing we'll ask about next time. Without the `detach()`, the record would
grow by one step's worth of arithmetic every time around the loop.

This also explains why standardizing mattered. The learned weights are for
the standardized columns, which is why the weight for size, 64, is so much
larger than the 2 per square meter we guessed in Listing 21-8: one unit of
the standardized column is about 34 square meters. And `base` has settled
on 256, the average price, because a house that is average in every column
has all-zero standardized features. Had we left the columns on their
original scales, the slope for size would have been thousands of times
larger than the slope for bedrooms, and no single learning rate would have
suited both.

A few facts about gradients that will save you confusion later. Only float
tensors have them: a count or a mask has no slope. A gradient is itself an
ordinary tensor with no record behind it, so Aura cannot take the gradient
of a gradient. And the first `gradient` call on a result does the backward
walk for everything behind it, so asking for `loss.gradient(base)` after
`loss.gradient(weights)` costs nothing extra.

## Summary

A tensor holds numbers of one dtype in a shape, on a device. The dtype is
checked by the compiler and the shape is checked when an operation runs. We
make tensors from arrays, from constants, and from seeded random numbers,
and read them back with `to_array()` and `item()`. Operations never change a
tensor: arithmetic broadcasts across matching shapes, reductions collapse an
axis, `matmul` turns rows of features into weighted sums, and masks and
`index_select` pick elements out.

A tensor marked with `requires_grad()` lets Aura compute the gradient of
anything derived from it, and stepping against the gradient, over and over,
is learning. Listing 21-13 is a complete training program. It is also
clumsy: we named every weight ourselves, rebuilt each one by hand after
every step, and would have to do it again for every new model. The next
chapter keeps the idea and replaces the bookkeeping with the pieces a real
training program is made of: parameters, layers, loss functions, and
optimizers.
