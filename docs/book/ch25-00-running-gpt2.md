# Running GPT-2

In 2019 OpenAI released GPT-2, a transformer trained on eight million web
pages to do the one thing our Shakespeare model does: predict the next
token. Its writing was good enough that the release made the news. It is
small by today's standards and no longer impressive next to its
successors, and that makes it the right model to study. Its weights are
public, they fit on any laptop, and its design is the one every later
model grew from.

In this chapter we'll run it. We won't call a library that hides the
model, because there is no such library to call. We'll download the weights
OpenAI trained, write the model in Aura, load the weights into it, and
have it continue any text we give it. By the end you'll have a
program of about 300 lines that is a complete implementation of GPT-2
inference, and you'll have written every line that is specific to the
model.

There are three jobs. The first is the weights. A trained model is a
file of named tensors, and we'll look inside the file, because its names
and shapes tell us what the model must look like. The second is the
tokenizer. GPT-2 does not read characters. It reads *tokens* that are
often whole words, made by a scheme called byte pair encoding, and to
talk to the model we have to cut text exactly the way its training data
was cut. The third is the model: the transformer of Chapter 24, with its
pieces named and arranged the way OpenAI did, so that the weights drop
into place. Then we put the three together and generate text.

Everything here runs on the CPU device. We use the smallest of the four
sizes GPT-2 was released in, with 124 million parameters. The others are
the same program with different numbers.
