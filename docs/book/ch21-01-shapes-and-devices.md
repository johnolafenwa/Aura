# Tensors, shapes, and devices

Suppose we want to predict what a house will sell for. We have a short list
of houses that sold recently, and for each one we know its size, how many
bedrooms it has, and how old it is. That is a table of numbers, one row per
house and one column per measurement, and by the end of the next chapter we
will have a program that learns the prices from it. First we need to get the
table into a form the `tensor` module can work with.

## Making a tensor

A tensor is made from an `Array`, and it is turned back into an `Array` when
we want to look at it:

*Filename: src/main.au*

```aura
from tensor import Device, Tensor

def main():
    device = Device.cpu()
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
    houses = Tensor[float32].from_array(table, device)
    print(houses.shape())
    print(houses.len())
    print(houses.device().name())
    print(houses.to_array())
```

<span class="caption">Listing 21-1: Six houses as a tensor</span>

```console
$ aura run src/main.au
[6, 3]
18
cpu
Array[float32](shape=[6, 3], values=[50.0, 1.0, 30.0, 80.0, 2.0, 12.0, 120.0, 3.0, 5.0, 65.0, 2.0, 40.0, 150.0, 4.0, 2.0, 95.0, 3.0, 20.0])
```

The `tensor` module is built in, so there is nothing to install or add to
`Aura.toml`. `Tensor[float32].from_array(table, device)` copies the array's
numbers to a device and gives us a tensor that holds them there. The
*shape*, `[6, 3]`, means what it meant for an `Array`: six rows of three
columns, stored row by row. People in machine learning call the number of
dimensions the *rank*, so this is a rank-two tensor, a matrix. A list of
numbers has rank one, and a batch of color images has rank four: image,
channel, row, column. Nearly every bug you will write in these chapters is a
shape that is not what you thought it was, so get used to printing
`shape()`.

Moving between an `Array` and a tensor is always written out. No operation
takes one where it expects the other, and `print` on a tensor itself shows
only a placeholder, because the numbers may be sitting in a graphics card's
memory. We look at a tensor by asking for a copy with `to_array()`. Aura
makes the trip visible because it can be the most expensive line in a
program, and a slow copy should never hide inside a `print`.

## The dtype is part of the type

The type in brackets is the tensor's *dtype*, the type of every element. It
can be `float32`, `float64`, `int32`, `int64`, or `bool`. Machine learning
runs almost entirely on `float32`: it is what graphics cards are fastest at,
and the numbers a model learns are not known to seven digits anyway. We will
use `int64` tensors for things that count or name, such as "this row is
class 2", and `bool` tensors for yes-or-no answers about each element.

In PyTorch the dtype is a property a tensor has while the program runs. In
Aura it is part of the type, like the `int64` in `list[int64]`, so mixing
dtypes is caught before the program starts:

```aura check-fail:AU2999
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    sizes = Tensor[float32].zeros([6], device)
    rooms = Tensor[int64].zeros([6], device)
    print((sizes + rooms).shape())
```

```console
$ aura check src/main.au
error[AU2999]: binary operator operands must match, found `tensor.Tensor[float32]` and `tensor.Tensor[int64]`
 --> src/main.au:7:12
  |
7 |     print((sizes + rooms).shape())
  |            ^
```

This is the rule from [Chapter 3](/book/ch03-02-data-types) that nothing
converts implicitly, applied to tensors. When we do want to combine the two
we convert one of them and say so: `rooms.to_float32()`. There is a member
for each numeric dtype: `to_int32()`, `to_int64()`, `to_float32()`, and
`to_float64()`.

The shape is different. It is not part of the type, because a program often
doesn't know it until it has read its data, so shapes are checked when an
operation runs. We'll see what that looks like in the next section.

## Tensors made from nothing

We won't always have an array to start from. The module can fill a tensor
for us:

```aura
import tensor
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    print(Tensor[float32].zeros([2, 3], device).to_array())
    print(Tensor[float32].full([4], 0.5, device).to_array())
    print(tensor.arange(5, device).to_array())
    print(tensor.rand([4], 7, device).to_array())
    print(tensor.randn([4], 7, device).to_array())
    print(tensor.randn([2], 7, device).to_array())
```

<span class="caption">Listing 21-2: Zeros, a constant, a count, and random numbers</span>

```console
$ aura run src/main.au
Array[float32](shape=[2, 3], values=[0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
Array[float32](shape=[4], values=[0.5, 0.5, 0.5, 0.5])
Array[int64](shape=[5], values=[0, 1, 2, 3, 4])
Array[float32](shape=[4], values=[0.77154654, 0.87084675, 0.11860371, 0.58119285])
Array[float32](shape=[4], values=[1.1829631, -0.43850836, -2.9015584, 0.27576596])
Array[float32](shape=[2], values=[1.1829631, -0.43850836])
```

`zeros` and `full` are called on `Tensor[float32]` because they need to know
the dtype. `arange(n, device)` counts from zero as a `Tensor[int64]`. The two
random functions give `float32` tensors: `rand` spreads its numbers evenly
between zero and one, and `randn` draws them from the bell curve centered on
zero, where most values fall between minus two and two.

The second argument to `rand` and `randn` is a *seed*. Random numbers matter
a great deal in machine learning, because every model starts as random
numbers, and Aura has a firm rule about them: a seed always gives the same
tensor. Run Listing 21-2 a hundred times, on any machine, on a processor or
on a graphics card, and it prints those same digits. The last line shows one
more property: a shorter tensor from the same seed is the beginning of the
longer one.

PyTorch has a hidden global generator that every random call advances, so
the numbers a line produces depend on every random call that ran before it.
Aura has no hidden generator. That costs us an argument on every call and
means we must remember to use a different seed for each tensor that should
differ. In return, a training run that went wrong can be run again and will
go wrong in exactly the same way, which is the only way such a thing ever
gets fixed.

## One number out

Sooner or later a computation ends in a single number we care about, such as
the average error of a model. `item()` reads the one element of a
one-element tensor as an ordinary Aura value:

```aura
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    ages = Tensor[float32].from_array(
        Array[float32].from_list([30.0, 12.0, 5.0, 40.0, 2.0, 20.0], [6]),
        device,
    )
    oldest = ages.max()
    print(oldest.shape())
    print(oldest.item())
    print(ages.item())
```

<span class="caption">Listing 21-3: `item()` reads a single element, and only a single element</span>

```console
$ aura run src/main.au
[1]
40.0
error[AU4007]: `tensor.item` expects a tensor of one element, found shape [6]
 --> src/main.au
  = note: Aura call chain (innermost first): Elements for tensor.Tensor.item at 1425:5 -> main at 3:1
```

`ages.max()` is itself a tensor, of shape `[1]`. That may seem fussy, but it
means the result is still on the device and can go straight into more
arithmetic without a trip back to the program. `item()` makes the trip. On a
tensor with more than one element it has no sensible answer, so the program
stops with `AU4007`, the code for every mistake in the shape of a tensor
operation. The note under the message is the chain of calls that led there,
innermost first; the first entry is inside the `tensor` module's own source,
and the last is our `main`.

Earlier chapters would reach for `aura verify` at this point, to prove that
the mistake cannot happen. It cannot help us here yet: the verifier does not
model tensors, so it checks no shapes, and a shape mistake is found by
running the program. The plain integer code around a model, such as the
arithmetic that sizes a batch or indexes a vocabulary list, is proved as it
is anywhere else.

## Devices

Every tensor so far has gone to `Device.cpu()`. A device is where a tensor's
memory is and where its arithmetic runs, and there are three:

```aura
from tensor import Device, Tensor

def main():
    match Device.cuda():
        case None:
            print("no NVIDIA GPU that Aura can use")
        case Device as gpu:
            print(f"found {gpu.name()}")
    best = Device.default()
    print(f"default device: {best.name()}, a GPU: {best.is_gpu()}")
    ages = Tensor[float32].full([3], 20.0, Device.cpu())
    moved = ages.to(best)
    print(moved.device().name())
```

<span class="caption">Listing 21-4: Asking which devices this machine has</span>

```console
$ aura run src/main.au
no NVIDIA GPU that Aura can use
default device: cpu, a GPU: false
cpu
```

That is the output on the Linux machine these chapters were written on, which
has no graphics card. `Device.cpu()` exists on every machine. `Device.metal()`
is the graphics processor of a Mac and `Device.cuda()` is an NVIDIA card, and
each returns `Device | None`, because the machine may not have one. Aura makes
us handle the `None`, the same way it makes us handle a missing dictionary
key. `Device.default()` is the convenient form: it picks a GPU when there is
one Aura can use and the processor otherwise, so on a recent Mac the second
line reads `default device: metal, a GPU: true`.

Two rules about devices are worth knowing from the start. A tensor stays on
the device it was made on, and both sides of an operation must be on the same
device; `to(device)` makes a copy somewhere else when we need one. And a
program gives the same answers wherever it runs: integers and masks come out
identical, and floats agree to within the last digit or two, because a
graphics card may add a long column of numbers in a different order.

For the next few chapters we will write `device = Device.cpu()` at the top of
`main` and pass `device` wherever a tensor is made. The programs are small
enough that a graphics card would not help, and writing the device once at
the top means changing that one line is all it will take when the programs
grow. [Chapter 26](/book/ch26-00-devices-and-speed) is about what happens
then.

Now that we can make tensors, let's do some arithmetic with them.
