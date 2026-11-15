# Coming from PyTorch

PyTorch is the library most machine learning code is written in. If you
go on in this field you will read a great deal of it, in papers'
repositories and in tutorials, whatever language you write yourself.
Aura's `tensor` module uses PyTorch's names on purpose, so that what you
learned here carries over in both directions.

Here is the training loop of Listing 23-3 in PyTorch, and then in Aura.

```python
model = Net()
optimizer = torch.optim.AdamW(model.parameters(), lr=0.01)
for step in range(1001):
    loss = F.cross_entropy(model(x), labels)
    optimizer.zero_grad()
    loss.backward()
    optimizer.step()
```

```aura fragment
mut model = Net(hidden=Linear(2, 16, 3, device), output=Linear(16, 2, 5, device))
mut optimizer = AdamW(model.parameters(), lr=0.01)
for step in range(1001):
    loss = tensor.cross_entropy(model.forward(x), labels)
    optimizer.zero_grad()
    loss.backward()
    optimizer.step()
```

They are nearly the same, line for line. The differences that remain
aren't accidents. Each follows from a rule that the rest of Aura already
has, and we have met every one of them in passing. This section gathers
them.

## The dtype is part of the type

In PyTorch a tensor is a `Tensor`, and whether it holds floats or
integers is found out when the program runs. In Aura it is a
`Tensor[float32]` or a `Tensor[int64]`, and the compiler knows which. A
function that takes token numbers says so in its signature, and adding a
float tensor to an integer tensor is rejected by `aura check`, before the
program runs.

Shapes are not part of the type. A shape mismatch is found when the
program runs, as in PyTorch, as an `AU4007` error that names both shapes.

## Nothing is global

PyTorch has a global random seed, set with `torch.manual_seed`. In Aura
the seed is an argument: `tensor.randn(shape, seed, device)`, `Linear(2,
16, seed, device)`. That is why every layer in these chapters carried a
number. A function that makes random tensors shows it in its parameters,
and two parts of a program can't disturb each other's random numbers.

The device is an argument for the same reason. There is no current
device to set, and a tensor never moves unless the program says
`to(device)`.

The one switch that is global is `tensor.set_grad_enabled`, which plays
the part of PyTorch's `no_grad()`.

## Tensors do not change, and models change only if `mut`

PyTorch has operations that modify a tensor in place, such as `x.add_(1)`
and assignment to a slice. Aura has none. Every operation makes a new
tensor, and a `Tensor` value never changes after it is made.

The one thing that changes is a `Parameter`, when an optimizer steps or
weights are loaded. Aura puts that under its ordinary rule for mutation:
`parameters()`, `load_state_dict()`, and the optimizer's `step()` need a
`mut` binding. So `model = Net(...)` without `mut` is a model that no
code can train or overwrite, and the compiler enforces it. In PyTorch
the same promise is a convention.

## `forward` is called by name

A PyTorch model is called like a function, `model(x)`, which runs its
`forward` method. In Aura you write `model.forward(x)`. A model is a
class with `impl Module`, not a subclass of a base class, and Aura
classes aren't callable.

`impl Module` with no body does what PyTorch's base class does by
inspecting the object when the program runs: it finds the parameters in
the fields and names them by their paths. Aura does it when the program
is compiled.

## Smaller differences

A layer's weight has the shape `[inputs, outputs]`, where PyTorch's
`nn.Linear` stores `[outputs, inputs]`. This matters when you load weights
saved from a PyTorch model: a `Linear` weight needs a `transpose()` on the
way in. GPT-2's file needed none because its layers were saved the other
way around to begin with.

`step()` clears the gradients it used. In PyTorch, forgetting
`zero_grad()` makes gradients pile up from step to step. In Aura the loop
still trains correctly.

`state_dict()` returns copies, so changing the dictionary does not change
the model, and `load_state_dict()` is how values go back in. Dropout is a
function, `tensor.dropout(x, p, seed)`, and like everything random it is
told its seed.

Weights are saved as safetensors and nothing else. PyTorch's own `.pt`
files can contain arbitrary Python objects, and Aura does not read them.
Most published models offer safetensors, and PyTorch can write the format.

## A phrase book

| PyTorch | Aura |
| --- | --- |
| `torch.tensor(data)` | `Tensor[float32].from_array(array, device)` |
| `torch.zeros(2, 3)` | `Tensor[float32].zeros([2, 3], device)` |
| `torch.randn(2, 3)` | `tensor.randn([2, 3], seed, device)` |
| `torch.arange(n)` | `tensor.arange(n, device)` |
| `x @ y` | `x.matmul(y)` |
| `x.T` | `x.transpose()` |
| `x[:, 2:5]` | `x.slice(1, 2, 5)` |
| `x[idx]` | `x.index_select(0, idx)` |
| `x.view(a, b)` | `x.reshape([a, b])` |
| `torch.cat([x, y], dim=1)` | `tensor.cat([x, y], 1)` |
| `torch.where(mask, x, y)` | `tensor.where(mask, x, y)` |
| `x < y` | `x.less(y)` |
| `x.numpy()` | `x.to_array()` |
| `x.item()` | `x.item()` |
| `x.to("cuda")` | `x.to(device)` |
| `with torch.no_grad():` | `tensor.set_grad_enabled(false)` |
| `model(x)` | `model.forward(x)` |
| `class Net(nn.Module)` | `class Net` and `impl Module for Net` |
| `nn.Linear(4, 8)` | `Linear(4, 8, seed, device)` |
| `F.cross_entropy(scores, labels)` | `tensor.cross_entropy(scores, labels)` |
| `torch.optim.AdamW(model.parameters(), lr=0.01)` | `AdamW(model.parameters(), lr=0.01)` |
| `model.train()`, `model.eval()` | the same |
| `torch.save(model.state_dict(), path)` | `tensor.save(path, names, tensors)` |

## What is not there yet

The `tensor` module is new, and PyTorch has had a decade and thousands
of contributors. You should know where the edges are before you plan a
project.

- There is no tokenizer in the standard library. We wrote GPT-2's in
  Chapter 25, and other models need theirs.
- There are no data loading tools. Batches are made by hand, as we did
  with `index_select`.
- Convolution and pooling work on two-dimensional images with square
  kernels. There are no one-dimensional or three-dimensional forms, no
  dilation, and no groups.
- There are no half-precision tensors. A file stored in 16-bit floats
  loads as `float32`, which doubles its size in memory.
- A program uses one GPU. There is no way to spread a model over several
  cards or machines.
- You can't yet write your own GPU operation. Everything is built from
  the operations the module provides.
- The CPU device is not tuned, as the last section said. Large models
  need a GPU to be practical.
- There is no GPU device on Windows.

None of these stood in the way of anything in these chapters, and a
complete GPT-2 fit in about 300 lines. But they are real, and for work
that depends on one of them, PyTorch is the tool to use today. The
[Manual](/manual/tensors) has the full list of operations and the exact
rules for each device.

## Summary

A program moves to a GPU by changing the one line that picks the device.
A Mac needs nothing more, and an NVIDIA card needs the driver and the
NVRTC and cuBLAS libraries. A GPU device waits until the program reads a
result and then runs the whole expression as one piece of work, so read
results rarely and read the loss after `step()`. Small tensors are faster
on the CPU, and large ones are far faster on a GPU, where Aura's speed is
level with PyTorch's or ahead of it.

Aura's tensor API follows PyTorch's names. It differs where Aura's own
rules apply: the dtype is in the type, the seed and the device are
arguments, tensors never change, and only a `mut` model can be trained.

That is the end of this part of the book. In six chapters we went from a
tensor of six house sizes to a running GPT-2. Along the way we fitted a
line by gradient descent, classified flowers, watched a model overfit and
reined it in, drew a circle with a hidden layer, recognized stripes with
convolutions, and trained a transformer to write something like
Shakespeare. The ideas underneath stayed the same throughout: put the
data in tensors, write a model as a function with adjustable numbers,
measure how wrong it is with a loss, and let the gradients say which way
to move. Everything else, including the largest models in the world, is
that loop with more layers.
