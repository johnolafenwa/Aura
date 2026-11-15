# Models and modules

In [Chapter 5](/book/ch05-00-classes) we grew a program from loose bindings
into a class, because the bindings belonged together and every function
kept taking all of them. The network in Listing 23-2 is in the same state:
two layers that travel everywhere as a pair. Let's give it a class.

## A model is a class

*Filename: src/main.au*

```aura
import tensor
from tensor import AdamW, Device, Linear, Module, Tensor

def ring(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Points in a square, labeled 1 inside the circle of radius 1 and 0 outside.
    x = tensor.rand([count, 2], seed, device) * 3.0 - 1.5
    inside = (x * x).sum(1).less(Tensor[float32].full([1], 1.0, device))
    labels = tensor.where(inside, Tensor[int64].full([1], 1, device), Tensor[int64].full([1], 0, device))
    return (x, labels)

def accuracy(scores: Tensor[float32], labels: Tensor[int64]) -> float32:
    right = scores.argmax().equal(labels)
    return tensor.where(right, scores.scalar(1.0), scores.scalar(0.0)).mean().item()

class Net:
    hidden: Linear
    output: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        return self.output.forward(self.hidden.forward(x).relu())

impl Module for Net

def main():
    device = Device.cpu()
    x, labels = ring(400, 1, device)
    held_x, held_labels = ring(400, 2, device)

    mut net = Net(hidden=Linear(2, 16, 3, device), output=Linear(16, 2, 5, device))
    mut optimizer = AdamW(net.parameters(), lr=0.01)
    for step in range(1001):
        loss = tensor.cross_entropy(net.forward(x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    print(f"held-out accuracy {accuracy(net.forward(held_x), held_labels):.3f}")

    mut total = 0
    for name, param in net.named_parameters():
        shape = param.value().shape()
        print(f"{name} {shape}")
        total += param.value().len()
    print(f"{total} numbers to learn")
```

<span class="caption">Listing 23-3: The network as a class that implements `Module`</span>

```console
$ aura run src/main.au
held-out accuracy 0.993
hidden.weight [2, 16]
hidden.bias [16]
output.weight [16, 2]
output.bias [2]
82 numbers to learn
```

`Net` is an ordinary class with two fields and one method. `forward` is the
model: it says how an input flows through the layers. There is nothing
special about the name as far as the compiler is concerned. We call it
`forward` because every layer in the module does and every PyTorch model
does.

The line that makes `Net` a model is `impl Module for Net`, with no body.
`Module` is a trait, in the sense of
[Chapter 10](/book/ch10-02-traits), and it gives a class the methods a
model needs: `parameters()` for an optimizer, `named_parameters()`, and a
few more that we are about to use. All of them are built on one required
method, `collect`, which names each field that holds weights. When we leave
the body out, the compiler writes `collect` for us by reading the class's
fields in order. It picks up any field that is a `Parameter[float32]`, any
field whose type implements `Module`, and any list of such a type, and it
ignores fields that hold no weights, such as a number or a string.

So `net.parameters()` found both layers, and the bug we worried about at
the end of the last section, a layer left out of the optimizer's list, is
gone. Add a third layer to the class and it trains without another line
changing. If a field holds weights in a shape the compiler does not know
how to walk, such as a dictionary of layers or a `Linear | None`, it
refuses to guess: that is error `AU2046`, and the fix is to write `collect`
by hand, which the [Manual](/manual/tensors#modules) shows.

`named_parameters()` gives each parameter with a name built from the field
names: `hidden.weight` is the `weight` of the layer in the field `hidden`.
The loop adds up `len()` over all of them: 82 numbers. That is the entire
model that found the circle. The count of parameters is the usual measure
of a model's size, and you will see it quoted in millions and billions.

If you know PyTorch, here is the one place where the spelling differs
enough to trip you. There, a model inherits from `nn.Module` and is called
like a function, `net(x)`. Aura has no inheritance, so a model *implements*
`Module`, and a class instance is not callable, so we write
`net.forward(x)`.

## A stack of layers

When a model is nothing more than layers applied one after another, there
is no need to write a class for it. `Sequential` takes a list of layers and
is itself a model:

```aura
import tensor
from tensor import AdamW, Device, Layer, Linear, ReLU, Sequential, Tensor

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

    mut net = Sequential([
        Linear(2, 16, 3, device) as dyn Layer,
        ReLU(),
        Linear(16, 16, 4, device),
        ReLU(),
        Linear(16, 2, 5, device),
    ])
    mut optimizer = AdamW(net.parameters(), lr=0.01)
    for step in range(1001):
        loss = tensor.cross_entropy(net.forward(x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    print(f"held-out accuracy {accuracy(net.forward(held_x), held_labels):.3f}")
    for name, param in net.named_parameters():
        print(f"{name} {param.value().shape()}")
```

<span class="caption">Listing 23-4: A deeper network as a `Sequential`</span>

```console
$ aura run src/main.au
held-out accuracy 0.980
0.weight [2, 16]
0.bias [16]
2.weight [16, 16]
2.bias [16]
4.weight [16, 2]
4.bias [2]
```

This network has two hidden layers. The list holds values of different
types, `Linear` and `ReLU`, so it is a list of trait objects,
`list[dyn Layer]`, as in [Section 10.5](/book/ch10-05-trait-objects). `Layer`
is the trait of anything with a `forward` from one float tensor to another.
Writing `as dyn Layer` on the first element tells the compiler the element
type, and the rest convert to match. `ReLU()` is the `relu` operation
dressed as a layer so it can sit in the list. It has no weights, which is
why the names skip from `0` to `2` to `4`: a `Sequential` names its layers
by position.

Use `Sequential` for a plain stack, and a class when the data takes any
other path: when a layer's output is used twice, or two paths are added
together, as they will be in the next chapter.

## Saving a model

Training can take hours. A model that exists only while the program runs
is not much use, so we need to write its weights to a file. `Module` gives
every model a `state_dict()`: a dictionary from each parameter's name to
the tensor it currently holds. The `tensor` module writes tensors in the
*safetensors* format, a simple file layout that PyTorch and the Hugging
Face libraries also read and write.

```aura
import tensor
from tensor import AdamW, Device, Linear, Module, Tensor

def ring(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # Points in a square, labeled 1 inside the circle of radius 1 and 0 outside.
    x = tensor.rand([count, 2], seed, device) * 3.0 - 1.5
    inside = (x * x).sum(1).less(Tensor[float32].full([1], 1.0, device))
    labels = tensor.where(inside, Tensor[int64].full([1], 1, device), Tensor[int64].full([1], 0, device))
    return (x, labels)

class Net:
    hidden: Linear
    output: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        return self.output.forward(self.hidden.forward(x).relu())

impl Module for Net

def save_model(model: Net, path: str) -> Result[int64, str]:
    state = model.state_dict()
    mut names: list[str] = []
    mut tensors: list[Tensor[float32]] = []
    for name in state.keys():
        view value = state[name]
        tensors.append(value.clone())
        names.append(name.clone())
    return tensor.save(path, names, tensors)

def main():
    device = Device.cpu()
    x, labels = ring(400, 1, device)
    mut net = Net(hidden=Linear(2, 16, 3, device), output=Linear(16, 2, 5, device))
    mut optimizer = AdamW(net.parameters(), lr=0.01)
    for step in range(1001):
        loss = tensor.cross_entropy(net.forward(x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()

    probe = Tensor[float32].from_array(Array[float32].from_list([0.2, -0.3, 1.4, 1.2], [2, 2]), device)
    print(net.forward(probe).to_array())
    match save_model(net, "ring.safetensors"):
        case Ok(size):
            print(f"wrote {size} bytes")
        case Err(message):
            print(f"could not save: {message}")
```

<span class="caption">Listing 23-5: Training a model and writing its weights to a file</span>

```console
$ aura run src/main.au
Array[float32](shape=[2, 2], values=[-11.8274145, 13.005921, 17.217714, -14.837989])
wrote 616 bytes
```

`tensor.save(path, names, tensors)` takes two lists of the same length and
returns a `Result`: the size of the file, or a message saying what went
wrong. Writing a file can always fail, and as everywhere in Aura since
[Chapter 9](/book/ch09-02-recoverable-errors), the failure is a value we
have to look at.

`save_model` turns the dictionary into those two lists. The loop is worth
a second look, because the obvious version of it does not compile. A tensor
is a handle to memory on a device, and Aura treats that handle as a
resource with one owner, like the file handles of
[Chapter 14](/book/ch14-00-resources-and-cleanup). `state.items()` would
have to duplicate every tensor in the dictionary to build its list of
pairs, and the compiler will not duplicate a resource behind our backs. So
we walk the keys, borrow each tensor in place with `view`, and ask for a
second handle explicitly with `clone()`, which, as we saw in Chapter 21,
copies no numbers.

Notice also that `save_model` takes the model as a plain parameter, with no
`mut`. Saving reads the weights and cannot change them, and the signature
says so. The tensors in a `state_dict()` are values: they are what the
weights were at that moment, and training the model further does not alter
them.

The file is 616 bytes: 82 numbers at four bytes each, plus a short header
that lists each tensor's name, dtype, and shape. Before saving, we printed
the model's scores for two test points so that we can check the next step.

## Loading it back

A weights file holds numbers and names. It does not hold the model: there
is no `forward` in it, and no record that the layers are joined by a
`relu`. To load a model we build one of the same shape in code, and then
replace its weights with the ones from the file.

```aura
import tensor
from tensor import Device, Linear, Module, Tensor

class Net:
    hidden: Linear
    output: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        return self.output.forward(self.hidden.forward(x).relu())

impl Module for Net

def load_model(model: mut Net, path: str, device: Device) -> Result[int64, str]:
    mut state = dict[str, Tensor[float32]]()
    for name in try tensor.weight_names(path):
        state[name.clone()] = try tensor.load_float32(path, name, device)
    return model.load_state_dict(state)

def report(outcome: Result[int64, str]) -> None:
    match outcome:
        case Ok(count):
            print(f"loaded {count} tensors")
        case Err(message):
            print(f"could not load: {message}")

def main():
    device = Device.cpu()
    probe = Tensor[float32].from_array(Array[float32].from_list([0.2, -0.3, 1.4, 1.2], [2, 2]), device)

    mut net = Net(hidden=Linear(2, 16, 100, device), output=Linear(16, 2, 200, device))
    print(net.forward(probe).to_array())
    report(load_model(net, "ring.safetensors", device))
    print(net.forward(probe).to_array())

    mut narrow = Net(hidden=Linear(2, 8, 100, device), output=Linear(8, 2, 200, device))
    report(load_model(narrow, "ring.safetensors", device))
    report(load_model(narrow, "rings.safetensors", device))
```

<span class="caption">Listing 23-6: Building the model again and loading its weights</span>

```console
$ aura run src/main.au
Array[float32](shape=[2, 2], values=[0.09174371, 0.10711506, -0.023798943, -0.17333642])
loaded 4 tensors
Array[float32](shape=[2, 2], values=[-11.8274145, 13.005921, 17.217714, -14.837989])
could not load: `load_state_dict` expects `hidden.weight` to have the shape [2, 8], found [2, 16]
could not load: cannot open `rings.safetensors`: No such file or directory (os error 2)
```

`tensor.weight_names(path)` lists the tensors in a file, and
`tensor.load_float32(path, name, device)` reads one of them onto a device.
Both return a `Result`, and `load_model` passes any failure up to its
caller with `try`. Then `load_state_dict` replaces each parameter with the
tensor of the same name, and reports how many it replaced.

The new model starts from different seeds, so its first answers for the two
test points are different numbers. After loading, they are the numbers
Listing 23-5 printed, digit for digit. The model is back.

This time the parameter is `model: mut Net`, and it has to be. Loading
weights changes the model as surely as training does, so
`load_state_dict` takes `mut self`, and a function that calls it must ask
its caller for mutable access. Compare the two signatures: `save_model`
promises not to touch the weights, `load_model` announces that it will, and
the compiler checks both.

The last two lines show what happens when things don't match. `narrow` has
8 hidden units where the file has 16, and `load_state_dict` refuses, naming
the tensor and both shapes. It also changes nothing when it refuses: a load
either replaces every parameter or none, so a failed load can't leave a
model half old and half new. The misspelled file name fails earlier, in
`weight_names`, with the operating system's message.

This is all it takes to use a model that somebody else trained, provided
we can write a class whose parameter names and shapes match their file. In
[Chapter 25](/book/ch25-00-running-gpt2) we will do exactly that with a
file half a gigabyte long.
