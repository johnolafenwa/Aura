# Devices and speed

Every program in the last five chapters began with `device = Device.cpu()`
and ran on an ordinary processor. That was the right choice for learning,
and for the sizes we used. It stopped being comfortable in Chapter 24,
where a small transformer took ten minutes to train, and in Chapter 25,
where GPT-2 wrote about one token a second.

Machine learning at real sizes runs on graphics processors. A GPU does
the same arithmetic as a CPU, on thousands of numbers at once, and the
large matrix products that fill a transformer are exactly the work it is
built for. Aura's tensors run on two kinds of GPU: the one in a Mac,
through Metal, and NVIDIA cards, through CUDA.

We'll start with what your machine needs, the one line of a program that
changes, and the one way in which a GPU behaves differently enough that
you should know about it. Then, for readers who know PyTorch or who will
read PyTorch code, which is most of the machine learning code there is,
we set the two side by side, say what is different in Aura and why, and
list what the `tensor` module does not have yet.
