# A transformer from scratch

The models that write text, the ones behind chat assistants and code
completion, are built on one design, the *transformer*. It was published in
2017 and almost every language model since has been a variation on it. It
has a reputation for being hard to understand. The design is about a
hundred lines of code on top of what we already have, and in this chapter
we'll write those lines.

Our model will be small: about 112,000 parameters, where the models in the
news have billions. We'll train it on the plays of Shakespeare, one
character at a time, on the CPU, in a few minutes. It will not write good
Shakespeare. It will start from random numbers, knowing nothing about
English or even that letters form words, and it will end up writing lines
that look like a play, with speaker names, line breaks, and mostly real
words. Watching it get there is the best way we know to see what these
models do.

First we turn text into numbers a model can
read and build the simplest model that predicts text at all, so that we
have a number to beat. Then we build *attention*, the one new idea in a
transformer, by hand. Then we assemble the whole model and train it. Last
we make it write, and look at the choices that go into picking each next
character.

The next chapter runs GPT-2, a real model with 124 million parameters
trained by OpenAI. It has the same parts as the one we're about to build,
in the same order. Nearly everything you write here, you'll write again
there with bigger numbers.
