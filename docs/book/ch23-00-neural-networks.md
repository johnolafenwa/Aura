# Neural networks

Every model in the last chapter was one `Linear` layer: a weighted sum of
its inputs. That was enough for house prices, where the truth was a
weighted sum, and for the flowers, where a straight boundary separated the
species well enough. For the wave in Listing 22-7 it was not enough, and we
had to compute the powers of `x` ourselves before a linear layer could bend.
We were doing the interesting part of the work by hand.

A *neural network* does that part itself. It is several layers applied one
after another, with a simple bend between them, and training adjusts all the
layers together so that the early ones learn to compute whatever the later
ones need. That is the whole idea, and it is less exotic than its name. The
models that recognize speech, translate languages, and write text are this
idea with more layers and more data.

We'll build three things in this chapter. The first is a small network that
learns a boundary no single layer can draw, and we'll print the boundary to
see that it did. The second is a proper home for a model: a class of our
own that holds its layers, can list its parameters by name, and can be saved
to a file and loaded back. The third is a *convolutional* network, the kind
used for images, which we'll teach to tell three kinds of picture apart
where a plain layer cannot beat guessing.
