# Convolutional networks

So far each row of input has been a short list of measurements, and the
order of the measurements carried no meaning. An image is different. Its
numbers are arranged in a grid, a pixel means something only in relation to
its neighbors, and the thing we are looking for can be anywhere in the
frame. In this section we'll build the kind of network that is designed
around those facts, and see why it is needed.

## Small pictures

Real image datasets are large downloads, so we'll draw our own. Each image
is 8 pixels by 8, mostly dark, with one bright line somewhere in it. The
line runs across, or down, or diagonally, and the job is to say which. To
keep it from being trivial, every pixel also gets some random noise.

*Filename: src/main.au*

```aura
import tensor
from tensor import AdamW, Device, Linear, Tensor

def lit(mask: Tensor[bool], device: Device) -> Tensor[float32]:
    return tensor.where(mask, Tensor[float32].full([1], 1.0, device), Tensor[float32].full([1], 0.0, device))

def stripes(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # 8 by 8 images with one bright line: across (0), down (1), or diagonal (2).
    kind = (tensor.rand([count, 1, 1], seed, device) * 3.0).to_int64()
    place = (tensor.rand([count, 1, 1], seed + 1, device) * 8.0).to_int64()
    row = tensor.arange(8, device).reshape([1, 8, 1])
    column = tensor.arange(8, device).reshape([1, 1, 8])
    full = [count, 8, 8]
    across = lit(row.equal(place).expand(full), device)
    down = lit(column.equal(place).expand(full), device)
    slant = column - row + 8
    diagonal = lit(slant.equal(place).expand(full), device) + lit(slant.equal(place + 8).expand(full), device)
    clean = (
        across * lit(kind.equal(kind.scalar(0)), device)
        + down * lit(kind.equal(kind.scalar(1)), device)
        + diagonal * lit(kind.equal(kind.scalar(2)), device)
    )
    images = clean + tensor.randn(full, seed + 2, device) * 0.3
    return (images.reshape([count, 1, 8, 8]), kind.reshape([count]))

def show(image: Tensor[float32]) -> None:
    pixels = image.reshape([8, 8]).to_array()
    for row in range(8):
        mut line = ""
        for column in range(8):
            if pixels[row, column] > 0.5:
                line += "#"
            else:
                line += "."
        print(line)

def accuracy(scores: Tensor[float32], labels: Tensor[int64]) -> float32:
    right = scores.argmax().equal(labels)
    return tensor.where(right, scores.scalar(1.0), scores.scalar(0.0)).mean().item()

def main():
    device = Device.cpu()
    x, labels = stripes(512, 8, device)
    held_x, held_labels = stripes(256, 2, device)
    print(x.shape())
    print(labels.slice(0, 0, 3).to_array())
    for at in range(3):
        show(x.slice(0, at, at + 1))
        print("")

    flat_x = x.reshape([512, 64])
    mut model = Linear(64, 3, 9, device)
    mut optimizer = AdamW(model.parameters(), lr=0.01)
    for step in range(301):
        loss = tensor.cross_entropy(model.forward(flat_x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    print(f"linear model: training {accuracy(model.forward(flat_x), labels):.3f}")
    print(f"linear model: held-out {accuracy(model.forward(held_x.reshape([256, 64])), held_labels):.3f}")
```

<span class="caption">Listing 23-7: Three kinds of picture, and a linear model that can't tell them apart</span>

```console
$ aura run src/main.au
[512, 1, 8, 8]
Array[int64](shape=[3], values=[0, 1, 2])
.......#
..#....#
....#...
........
........
.#......
........
########

.......#
...#...#
.......#
.##....#
.......#
.......#
..#....#
.......#

........
..#.....
...#....
....#...
.....#..
......#.
.......#
#.......

linear model: training 0.551
linear model: held-out 0.348
```

`stripes` is the longest piece of tensor arithmetic we've written, and it
is all operations from Chapter 21. Each image gets a random `kind` from 0
to 2 and a random `place` from 0 to 7. `row` and `column` are the numbers 0
to 7 laid along two different axes, so comparing `row` with `place`
broadcasts to a mask that is true along one whole row of each image, which
is a line running across. The diagonal compares `column - row` with the
place, with a second term so that a diagonal that runs off one edge comes
back on the other. Multiplying each pattern by "is this image of that
kind" keeps exactly one of the three, and `randn` adds the noise.

The result has shape `[512, 1, 8, 8]`. This is the layout the image layers
expect: images, then *channels*, then rows, then columns. A grayscale image
has one channel, and a color image has three, for red, green, and blue.
`show` prints one image, with `#` for every pixel brighter than 0.5. The
first three are a line across, a line down, and a diagonal, and you can see
the noise knocking out a pixel here and adding one there.

Then we try the obvious thing: flatten each image into a row of 64 numbers
and train the classifier from Chapter 22 on it. It gets about half of the
training images right and 35% of the held-out images. With three kinds to
choose from, guessing scores 33%.

A plain layer fails here for a reason worth understanding. Flattening
threw away the grid. To the linear model, pixel 12 and pixel 13 are two
unrelated inputs, and it has no way to know that they are neighbors, or
that pixel 20 sits directly below pixel 12. Worse, a line across in row 2
and a line across in row 6 light up entirely different inputs, so what the
model learns about one tells it nothing about the other. Every pixel is lit
equally often in every kind of image, and "which pixels are bright" carries
no information at all. What tells the kinds apart is the *arrangement* of
bright pixels relative to each other.

## Convolution

A *convolution* is a layer built for arrangements. It has a small grid of
weights, called a *kernel*, typically 3 pixels by 3. The layer slides the
kernel over the image, and at every position it multiplies the nine weights
by the nine pixels underneath and adds them up. The result is a new grid,
with a large value wherever the patch of image under the kernel resembled
the kernel's pattern. A kernel with large weights across its middle row
responds to short horizontal strokes, wherever they are.

Two properties follow, and they are exactly the ones the linear model
lacked. The kernel only ever looks at neighboring pixels, so the grid
matters. And the same nine weights are used at every position, so a stroke
learned in one corner is recognized in every other. This sharing is also
why convolutional layers are small: nine weights, where a linear layer over
the same image needs 64 for every output.

A layer has many kernels, each producing its own output grid, called a
channel. Nobody designs the kernels. They start random, like every other
weight, and training shapes them into whatever detectors lower the loss.

Let's follow an image through the layers to see what happens to its shape:

```aura
import tensor
from tensor import Conv2d, Device, Tensor

def main():
    device = Device.cpu()
    images = tensor.randn([64, 1, 8, 8], 1, device)
    first = Conv2d(1, 8, 3, 1, 1, 3, device)
    second = Conv2d(8, 16, 3, 1, 1, 5, device)
    print(first.weight.value().shape())

    strokes = first.forward(images).relu()
    print(strokes.shape())
    smaller = tensor.max_pool2d(strokes, 2, 2)
    print(smaller.shape())
    shapes = second.forward(smaller).relu()
    print(shapes.shape())
    size = shapes.shape()
    summary = shapes.reshape([size[0], size[1], size[2] * size[3]]).mean(2)
    print(summary.shape())
```

<span class="caption">Listing 23-8: The shape of a batch of images at each stage</span>

```console
$ aura run src/main.au
[8, 1, 3, 3]
[64, 8, 8, 8]
[64, 8, 4, 4]
[64, 16, 4, 4]
[64, 16]
```

`Conv2d(1, 8, 3, 1, 1, 3, device)` takes, in order: the number of input
channels, the number of output channels, the kernel size, the stride, the
padding, a seed, and the device. So `first` reads 1 channel and produces 8,
with 3 by 3 kernels, and its weight has shape `[8, 1, 3, 3]`: eight
kernels, each one channel deep. The *stride* is how far the kernel moves at
each step, and 1 means it visits every position. The *padding* is a border
of zeros added around the image so the kernel can be centered on the edge
pixels. With a 3 by 3 kernel, a padding of 1 keeps the output the same size
as the input, which is why `strokes` is still 8 by 8.

`tensor.max_pool2d(strokes, 2, 2)` is *pooling*. It divides each grid into
2 by 2 blocks and keeps the largest value in each, halving the height and
the width. It has no weights. Pooling keeps the fact that a stroke was
found and discards exactly where, to within a pixel, and it means the next
layer's 3 by 3 kernels cover a 6 by 6 area of the original image. So the
first layer finds strokes, and the second, looking at a wider area of the
first layer's output, finds arrangements of strokes. Deep networks for real
photographs repeat this pattern many times, and their later layers respond
to textures, then parts, then whole objects.

The second convolution reads the 8 channels and produces 16. Its kernels
are 8 channels deep, so each one looks at a 3 by 3 patch of all eight
stroke maps at once.

The last step averages each of the 16 channels over its 4 by 4 grid,
leaving 16 numbers per image: how strongly each of the 16 learned patterns
appeared, anywhere. That is a row of measurements, and we know what to do
with those. A `Linear` layer from 16 to 3 turns them into scores.

## Training it

```aura
import tensor
from tensor import AdamW, BatchNorm2d, Conv2d, Device, Linear, Module, Tensor

def lit(mask: Tensor[bool], device: Device) -> Tensor[float32]:
    return tensor.where(mask, Tensor[float32].full([1], 1.0, device), Tensor[float32].full([1], 0.0, device))

def stripes(count: int64, seed: int64, device: Device) -> (Tensor[float32], Tensor[int64]):
    # 8 by 8 images with one bright line: across (0), down (1), or diagonal (2).
    kind = (tensor.rand([count, 1, 1], seed, device) * 3.0).to_int64()
    place = (tensor.rand([count, 1, 1], seed + 1, device) * 8.0).to_int64()
    row = tensor.arange(8, device).reshape([1, 8, 1])
    column = tensor.arange(8, device).reshape([1, 1, 8])
    full = [count, 8, 8]
    across = lit(row.equal(place).expand(full), device)
    down = lit(column.equal(place).expand(full), device)
    slant = column - row + 8
    diagonal = lit(slant.equal(place).expand(full), device) + lit(slant.equal(place + 8).expand(full), device)
    clean = (
        across * lit(kind.equal(kind.scalar(0)), device)
        + down * lit(kind.equal(kind.scalar(1)), device)
        + diagonal * lit(kind.equal(kind.scalar(2)), device)
    )
    images = clean + tensor.randn(full, seed + 2, device) * 0.3
    return (images.reshape([count, 1, 8, 8]), kind.reshape([count]))

def accuracy(scores: Tensor[float32], labels: Tensor[int64]) -> float32:
    right = scores.argmax().equal(labels)
    return tensor.where(right, scores.scalar(1.0), scores.scalar(0.0)).mean().item()

class Cnn:
    first: Conv2d
    second: Conv2d
    norm: BatchNorm2d
    head: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        strokes = tensor.max_pool2d(self.first.forward(x).relu(), 2, 2)
        shapes = self.norm.forward(self.second.forward(strokes)).relu()
        size = shapes.shape()
        summary = shapes.reshape([size[0], size[1], size[2] * size[3]]).mean(2)
        return self.head.forward(summary)

impl Module for Cnn

def one_at_a_time(model: Cnn, x: Tensor[float32], labels: Tensor[int64]) -> int64:
    mut right = 0
    for at in range(50):
        pick = model.forward(x.slice(0, at, at + 1)).argmax().item()
        if pick == labels.slice(0, at, at + 1).item():
            right += 1
    return right

def main():
    device = Device.cpu()
    x, labels = stripes(512, 8, device)
    held_x, held_labels = stripes(256, 2, device)
    mut model = Cnn(
        first=Conv2d(1, 8, 3, 1, 1, 3, device),
        second=Conv2d(8, 16, 3, 1, 1, 5, device),
        norm=BatchNorm2d(16, device),
        head=Linear(16, 3, 7, device),
    )
    mut optimizer = AdamW(model.parameters(), lr=0.01)
    for step in range(101):
        at = (step * 64) % 448
        scores = model.forward(x.slice(0, at, at + 64))
        loss = tensor.cross_entropy(scores, labels.slice(0, at, at + 64))
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 25 == 0:
            print(f"step {step}: loss {loss.item():.3f}")
    print(f"held-out accuracy {accuracy(model.forward(held_x), held_labels):.3f}")

    print(f"training mode, one image at a time: {one_at_a_time(model, held_x, held_labels)} of 50")
    model.eval()
    print(f"evaluation mode, one image at a time: {one_at_a_time(model, held_x, held_labels)} of 50")
```

<span class="caption">Listing 23-9: A convolutional network for the three kinds of picture</span>

```console
$ aura run src/main.au
step 0: loss 1.163
step 25: loss 0.193
step 50: loss 0.025
step 75: loss 0.013
step 100: loss 0.006
held-out accuracy 1.000
training mode, one image at a time: 22 of 50
evaluation mode, one image at a time: 50 of 50
```

A hundred steps, on batches of 64 images taken in order with `slice`, and
the model gets every one of the 256 held-out images right, where the
linear model was guessing. The `Cnn` class is the pipeline from Listing
23-8 with one addition, and `impl Module for Cnn` collects the parameters
of all four fields.

## Batch normalization, and the two modes

The addition is `BatchNorm2d`, a *batch normalization* layer. In Chapter
21 we standardized our inputs so that every column had mean zero and
spread one, and training went better for it. Batch normalization does the
same thing in the middle of a network: for each channel, it subtracts the
mean and divides by the spread, computed over the current batch, and then
applies a learned scale and shift. It keeps the numbers flowing between
layers in a reasonable range however the earlier layers change, and deep
convolutional networks train much faster and more reliably with it.

It also introduces a trap, and the last two lines of output show it.
"Computed over the current batch" is fine during training, when batches
have 64 images. Now suppose the trained model is asked about one image.
The batch is that image alone, its statistics describe nothing but itself,
and normalizing by them wipes out the very differences the network
learned to look for. Asked about held-out images one at a time, the model
that scored 100% a moment ago gets 22 of 50.

The answer is that a batch normalization layer works in two modes. In
*training mode*, the mode it starts in, it uses the batch's own statistics,
and meanwhile keeps a running average of them. In *evaluation mode* it
uses those running averages, which don't depend on what else is in the
batch. `model.eval()` switches every layer of a model to evaluation mode,
and `model.train()` switches back. After `model.eval()`, the same 50
images, one at a time, are all classified correctly.

Both methods come from `Module` and take `mut self`, because changing how
a model computes is a change to the model. The habit to form is simple and
it matters: call `model.eval()` before using a model for anything other
than training, and `model.train()` before training it again. Forgetting it
is one of the commonest reasons a model that tested well behaves strangely
once deployed.

The running averages are saved by `state_dict()` and restored by
`load_state_dict()`, so a loaded model has them, but they are not
parameters: `parameters()` leaves them out, and no optimizer changes them.
They are updated as a side effect of `forward` in training mode, which is
the one place a layer in the `tensor` module changes what it holds through
a model that is not `mut`. The weights themselves still obey the rule from
Chapter 22.

Convolution here is two-dimensional with square kernels, and one stride
and one padding for both directions. `AvgPool2d`, `MaxPool2d`, and
`Flatten` exist as layers for use in a `Sequential`, and the
[Manual](/manual/tensors#layers) has the full list.

## Summary

A neural network is layers with an activation between them, trained
together by the same four-line loop. The hidden layers compute features
that nobody specified, and the gradient reaches every layer because the
loss depends on all of them. One hidden layer of 16 units was enough to
learn a circle that no linear model could.

A model is a class whose fields are layers. `impl Module for Net` with no
body makes the compiler find the parameters, which gives the class
`parameters()`, `named_parameters()`, `state_dict()`,
`load_state_dict()`, `train()`, and `eval()`. `Sequential` is a ready-made
model for a plain stack. Weights are saved and loaded as safetensors
files, and a model is restored by building the same class and loading the
file into it. The methods that can change a model take `mut self`, and the
ones that only read it don't.

For images, convolution shares a small kernel across every position,
pooling shrinks the grid, and batch normalization keeps the numbers in
range, at the price of remembering `eval()`.

Images have a grid. Text has an order, and the meaning of a word depends
on words that may be far away from it. The next chapter builds the
architecture designed for that, the transformer, from an empty file to a
model that writes.
