# Tensors

The rest of this book is about machine learning: programs that are not told
the rule they should follow, and work it out from examples. A spam filter is
not a list of forbidden words that somebody typed in. It is a few thousand
numbers that were nudged, one batch of labeled mail after another, until
multiplying a message by them gave the right answer most of the time. The
text generators that have become so familiar are the same idea with more
numbers. Over the next five chapters we will build that machinery from the
bottom: fit a line to six points, grow the line into a neural network, grow
the network into a small transformer that writes passable Shakespeare, and
then load the published weights of GPT-2 and have it finish our sentences.

We don't assume you have done any of this before. We do assume you have read
[Section 8.5](/book/ch08-05-numeric-arrays), because everything here stands on
the `Array` type from that section, and that you are willing to meet a little
arithmetic. There is no calculus to do by hand. Aura does the calculus, and
we will spend our time on what the numbers mean.

All of it is built from one type, `Tensor[T]`, which lives in the `tensor`
module. A tensor is an `Array` with two additions. It lives on a *device*,
which can be the processor or a graphics card, so the same program can run on
either. And it can remember how it was computed, which is what lets Aura work
out how to change a model's numbers to make its answers better. This chapter
covers those in order: what a tensor is and where it lives, the operations
that combine tensors, and gradients, the idea that makes learning possible.

If you have used PyTorch, most of the names will be familiar on purpose:
`Tensor`, `matmul`, `backward()`, `Linear`, `parameters()`. A few things are
different, and each difference comes from a rule Aura already had. A tensor's
element type is part of its type, a tensor never changes after it is made, and
there is no global state: the device and the random seed are always arguments.
We'll point these out as we meet them, and
[Chapter 26](/book/ch26-00-devices-and-speed) collects them in one place.

Every program in these chapters runs on the processor of an ordinary laptop,
most of them in a second or two. A graphics card makes the larger ones much
faster, and we'll get to that, but you don't need one to follow along.
