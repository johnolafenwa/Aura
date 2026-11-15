# Linear regression

Listing 21-13 priced a house as a weighted sum of its measurements plus a
base amount. That model has a name, *linear regression*, and it is the
oldest and most used model there is. We'll rebuild it here with the proper
tools, and then train it on enough houses to find out whether it recovers a
rule we hid in the data.

## Parameters and layers

Here is the program from Listing 21-13 again, with the same six houses and
the same number of steps:

*Filename: src/main.au*

```aura
from tensor import Device, Linear, SGD, Tensor

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
    mut model = Linear(3, 1, 7, device)
    mut optimizer = SGD(model.parameters(), lr=0.1)
    for step in range(301):
        error = model.forward(x) - prices
        loss = (error * error).mean()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 100 == 0:
            print(f"step {step}: loss {loss.item()}")
    print(model.weight.value().to_array())
    print(model.bias.value().item())
```

<span class="caption">Listing 22-1: The house model with a layer and an optimizer</span>

```console
$ aura run src/main.au
step 0: loss 74264.2
step 100: loss 29.759802
step 200: loss 15.935195
step 300: loss 8.878393
Array[float32](shape=[3, 1], values=[64.042725, 16.34084, -16.027868])
255.99998
```

The loss ends within a hair of where Listing 21-13 ended, and the weights
agree to three digits. The small difference is the starting point: these
weights began as small random numbers, not zeros. Otherwise this is the same
computation. The names are PyTorch's, and
if you have trained a model there, the loop reads exactly as you would
expect. Let's go through the new pieces.

`Linear(3, 1, 7, device)` is a *layer*: a small class that owns some weights
and knows what to do with them. This one takes rows of 3 numbers in and
gives rows of 1 number out. Inside it are a `weight` of shape `[3, 1]` and a
`bias` of shape `[1]`, and `model.forward(x)` computes
`x.matmul(weight) + bias`, which is the line we wrote by hand before. "Bias"
is the usual name for what we called the base. The `7` is a seed. A layer
starts with small random weights, and like every random thing in Aura they
come from a seed we give, so the run above will be identical on your
machine.

`weight` and `bias` are not plain tensors. Each is a `Parameter[float32]`,
and a parameter is the one kind of tensor that can change. It holds a float
tensor, hands it out with `value()`, and can have it replaced. Anything
computed from `value()` is tracked, as if we had called `requires_grad()`,
so parameters are also the tensors gradients are taken with respect to.

`loss.backward()` computes the gradient of the loss with respect to every
parameter that contributed to it, and stores each gradient next to its
parameter, where `model.weight.grad` can read it. We no longer ask for
gradients one at a time, and we don't need to know how many parameters the
model has.

`SGD(model.parameters(), lr=0.1)` is an *optimizer*. The name stands for
stochastic gradient descent, and it is the rule we wrote by hand:
`optimizer.step()` replaces each parameter with itself minus the learning
rate times its gradient. `optimizer.zero_grad()` clears the stored gradients
before the new ones are computed.

## The four lines

Every training loop from here to the end of the book has this body:

```aura fragment
loss = how_wrong(model.forward(inputs), targets)
optimizer.zero_grad()
loss.backward()
optimizer.step()
```

Run the model forward, measure the loss, send the gradients backward, take
a step. The model will change, the loss function will change, the inputs
will come from different places, and these four lines will stay.

Two details differ from PyTorch, and both are in our favor. In PyTorch,
gradients pile up from one `backward()` to the next until `zero_grad()`
clears them, and forgetting the call is a classic bug: the model trains on
the sum of every gradient it has ever seen. In Aura, `step()` clears the
gradients it used, so the loop is correct even without `zero_grad()`. We
keep the call anyway, because it is what readers of training code expect to
see. The second detail is where to read the loss. We call `loss.item()`
after `step()`, not between `backward()` and `step()`. On the processor it
makes no difference. On a graphics card the forward pass, the gradients, and
the update all run together inside `step()`, and reading the loss before
that would run the forward pass a second time.
[Chapter 26](/book/ch26-01-running-on-a-gpu) explains why.

## Only a `mut` model can be trained

Look at the two `mut` keywords in Listing 22-1. Neither is decoration. Let's
take the first one away:

```aura check-fail:AU3003
from tensor import Device, Linear, SGD

def main():
    device = Device.cpu()
    model = Linear(3, 1, 7, device)
    mut optimizer = SGD(model.parameters(), lr=0.1)
    optimizer.step()
```

```console
$ aura check src/main.au
error[AU3003]: method `parameters` requires a mutable receiver
 --> src/main.au:6:31
  |
6 |     mut optimizer = SGD(model.parameters(), lr=0.1)
  |                               ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

`parameters()` takes `mut self`. It hands the optimizer the means to change
the model's weights, and Aura treats that like any other mutation: you may
only do it to something you declared `mut`. `step()` and `zero_grad()` take
`mut self` too, which is why the optimizer is `mut` as well.

This gives a guarantee that is worth stating plainly, because Python has no
equivalent: *the weights of a model that is not declared `mut` never
change*. If a function receives a model as an ordinary parameter, with
shared access, nothing that function calls can train it, corrupt it, or
quietly fine-tune it. When we load a published model in
[Chapter 25](/book/ch25-00-running-gpt2) and only want its predictions, we
will bind it without `mut` where we can, and the compiler will hold us to
that.

`loss.backward()` is the single exception to "you can see every effect in
the types". It writes gradients next to the parameters without a `mut`
anywhere. A gradient is working storage, though: it never changes what
`forward` computes, and nothing happens to the weights until a `mut`
optimizer takes a step.

## More houses, and a hidden rule

Six houses were enough to show the mechanics. They are not enough to trust.
To see whether training finds the truth, we need data where we know the
truth, so let's manufacture some. The function below invents any number of
houses with a random size, bedroom count, and age, and prices each one by a
fixed rule: 2.1 per square meter, 14 per bedroom, minus 0.9 per year, plus
48. Then it adds noise to every price, a random amount that is typically 8
in either direction, to stand in for everything about a real house that our
three columns don't capture.

```aura
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

def main():
    device = Device.cpu()
    x, prices = make_houses(200, 1, device)
    print(x.slice(0, 0, 2).to_array())
    print(prices.slice(0, 0, 2).to_array())

    mean = x.mean(0)
    centered = x - mean
    spread = (centered * centered).mean(0).sqrt()
    scaled = centered / spread

    mut model = Linear(3, 1, 7, device)
    mut optimizer = SGD(model.parameters(), lr=0.1)
    for step in range(121):
        error = model.forward(scaled) - prices
        loss = (error * error).mean()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 40 == 0:
            print(f"step {step}: loss {loss.item()}")

    per_unit = model.weight.value().reshape([3]) / spread
    base = model.bias.value() - (per_unit * mean).sum()
    print(per_unit.to_array())
    print(base.item())
```

<span class="caption">Listing 22-2: Recovering a rule from 200 noisy examples</span>

```console
$ aura run src/main.au
Array[float32](shape=[2, 3], values=[91.783356, 1.0, 48.544098, 95.08807, 3.0, 17.991093])
Array[float32](shape=[2, 1], values=[207.451, 268.1046])
step 0: loss 81543.164
step 40: loss 63.06055
step 80: loss 63.058975
step 120: loss 63.05898
Array[float32](shape=[3], values=[2.1054823, 13.242224, -0.88937664])
49.090485
```

`make_houses` is ordinary tensor arithmetic. Each column starts as random
numbers between zero and one and is stretched to a sensible range; the
bedroom column goes through `to_int64()` to cut off its fraction, which
turns it into whole numbers from 1 to 5. `tensor.cat(tensors, 1)` joins the
three columns side by side along axis 1, giving the `[200, 3]` table. The
function returns the table and the prices as a tuple, and `main` unpacks it.

The last four lines of `main` undo the standardizing. The model learned
weights for the scaled columns, and dividing each by its column's spread
turns it back into "price per square meter" and so on. The result is the
part to look at: 2.11 per square meter, 13.2 per bedroom, minus 0.89 per
year, and a base of 49.1. The rule we hid was 2.1, 14, minus 0.9, and 48.
From 200 noisy examples and no hints, gradient descent has read our rule
back to us, off by a few percent.

Now look at the loss. It fell to 63 within 40 steps and then stopped
moving. It will not go lower however long we train, and it shouldn't. We
added noise with a typical size of 8 to every price, and 8 squared is 64.
The model has learned everything that can be learned from these three
columns, and what remains is the part of each price that they don't
explain. Every real dataset has a floor like this. A loss that reaches it
is a success, and one of the skills in this field is telling a model that
has hit the floor from a model that is stuck.

A model that outputs a number is doing regression, and the mean squared
error is its natural loss. Plenty of questions are not "how much" but
"which one", and for those we need a different kind of answer.
