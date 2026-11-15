# Learning from data

At the end of the last chapter we had a program that learned to price six
houses. It did the right thing in the most laborious way possible. We
created each weight by hand, asked for each gradient by name, and rebuilt
every weight after every step. A model with a hundred tensors of weights
would need a hundred of each of those lines.

This chapter rewrites that program the way training programs are really
written, and then asks more of it. First we replace the bookkeeping with
three pieces from the `tensor` module: a `Parameter`, which is a tensor that
is allowed to change; a layer, which owns parameters and knows how to use
them; and an optimizer, which takes the downhill steps. The loop that comes
out is four lines long and will not change shape again for the rest of the
book, whether the model has four numbers or a hundred million.

Then we use it. We'll predict a number, which is called *regression*, and
then predict a category, which is called *classification* and needs a
different way to measure being wrong. Last, we'll look at the questions that
decide whether a trained model is any good: how to feed it data a batch at a
time, how to tell whether it learned the pattern or memorized the examples,
and what to do about it when it memorized.

The models stay tiny. Everything in this chapter is a single matrix product
and an addition. That is deliberate: the loop, the loss, and the habits
around them are the same for every model, and they are easier to see when
the model is not in the way.
