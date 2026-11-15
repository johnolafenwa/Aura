# Book Chapters On Tensors And Machine Learning

## Goal

John asked on 2026-11-15 for the book to gain "very strong ml and tensor
programming content covering the basics of ml and programming them in
aura, building mini inference with gpt2 and other sensible things". This
note covers chapters 21 to 26. The other gaps from the documentation audit
are in `2026-11-15-book-library-sections.md`.

## Work Completed

- Chapter 21, tensors: shapes, dtypes, devices, operations, broadcasting,
  and gradients, ending with gradient descent written by hand.
- Chapter 22, learning from data: linear regression, classification with
  cross-entropy, batches, held-out data, overfitting, weight decay, and
  `AdamW` against `SGD`.
- Chapter 23, neural networks: hidden layers and activations, a model as a
  class with `impl Module`, `Sequential`, saving and loading safetensors
  files, and a convolutional network with batch norm and its two modes.
- Chapter 24, a transformer from scratch: a character vocabulary, a table
  baseline, attention written out by hand, a two-block model of 112,577
  parameters trained on the tiny Shakespeare file, and generation with a
  temperature.
- Chapter 25, running GPT-2: listing the published weight file, a byte
  pair tokenizer that reads `vocab.json` and `merges.txt`, the model with
  OpenAI's field names so that `load_state_dict` places all 148 tensors,
  and top-k sampling.
- Chapter 26, devices and speed: what Metal and CUDA need, when GPU work
  runs, the recorded A40 and M2 timings from `benchmarks/tensor`, the
  differences from PyTorch, and what the module does not have.
- `index.md` and `OUTLINE.md` describe the new part.

## Verification

- Every listing with output was run with the released 0.16.0 Linux x86-64
  compiler on the CPU device and the printed output was written into the
  page from the run. The long listings were run with `aura run --backend
  direct`. Listing 24-4 trains for about ten minutes on four cores.
- Listings shown as fragments in 24.3, 24.4, and 25.4 were compared line
  for line with the complete programs that produced the output.
- The tokenizer of 25.2 gave the same token numbers as the reference
  `tokenizers` implementation on 313 texts.
- GPT-2 was loaded from `openai-community/gpt2` on Hugging Face. Its
  greedy continuation of "Hello, I'm a language model," decodes to " not a
  programming language. I'm a language model".
- `scripts/check_doc_examples.py` passes on all six chapters.
- Nothing in these chapters was run on a GPU for this work. The GPU
  figures in 26.1 are the ones recorded in `benchmarks/tensor/README.md`.

## Found And Not Fixed

- Plain `aura run` is very slow on long loops over host data. Building the
  character vocabulary over 1.1 million characters takes 14 seconds
  against 1 second with `--backend direct`, and loading GPT-2's tokenizer
  tables did not finish in two minutes against 1 second natively.
- The CPU device trains a 112,577-parameter transformer at about 0.4
  seconds a step (32 windows of 64 tokens), and GPT-2 writes a token in
  about 0.8 seconds.
- `regex` has no lookahead, which GPT-2's splitting pattern uses. The
  chapter works around it in code.
- A `Tensor[bool]` cannot be converted to a float tensor; the chapters use
  `where`.
- `items()` on a `dict[str, Tensor[float32]]` is error `AU3007`; the
  chapters loop over `keys()` and clone the value.
- `matmul` with a rank-one operand has no gradient.
- Adding tensors of two dtypes is reported as `AU2999` (unsupported), not
  as a type mismatch.
- `print(tensor)` prints `tensor.Tensor(node=<tensor-node>)`.
- There is no tokenizer and no generation helper in the standard library.
