# Making it write

A trained model is a function from some text to 65 scores for the next
character. To write, we call it in a loop: ask for the scores, choose a
character, add it to the text, and ask again with the longer text. Each
character the model writes becomes part of what it reads next.

## Loading the model and choosing characters

We'll extend the program of Listing 24-4 so that it can do two things:
train and save when we pass it the word `train`, and otherwise load the
saved weights and write. Add `import sys` to the imports, add these three
functions, and replace `main`.

*Filename: src/main.au*

```aura fragment
def load_model(model: mut Transformer, path: str, device: Device) -> Result[int64, str]:
    mut state = dict[str, Tensor[float32]]()
    for name in try tensor.weight_names(path):
        state[name.clone()] = try tensor.load_float32(path, name, device)
    return model.load_state_dict(state)

def pick(scores: Tensor[float32], rng: mut random.Rng) -> int64:
    # Draws one character number, each with the chance the scores give it.
    chances = scores.softmax().to_array()
    mut left = rng.next_float()
    for at in range(chances.len()):
        left -= chances[at] as float64
        if left <= 0.0:
            return at
    return chances.len() - 1

def generate(model: Transformer, start: list[int64], count: int64, context: int64, temperature: float64, rng: mut random.Rng, device: Device) -> list[int64]:
    mut ids = start.copy()
    for step in range(count):
        mut from = 0
        if ids.len() > context:
            from = ids.len() - context
        window = ids[from:]
        length = window.len()
        scores = model.forward(as_tensor(window, device).reshape([1, length]))
        last = scores.slice(1, length - 1, length).reshape([-1])
        ids.append(pick(last / temperature, rng))
    return ids
```

<span class="caption">Listing 24-5: Loading the saved weights, and writing one character at a time</span>

`load_model` is the function from Chapter 23 with the model's type
changed. `pick` is the function from Listing 24-2 that draws a character
with the chances the scores give it.

`generate` is the loop. The model can read at most `context` characters,
because it has position vectors for 64 places and no more, so when the
text grows longer than that we pass only the last 64. `forward` wants a
batch, so we give it a batch of one row with `reshape([1, length])`. It
returns scores for every position, each predicting the character after
it. We only want the prediction after the last character, which `slice`
cuts out.

Every step runs the whole model on the whole window again, although all
but one of its characters were there the step before. That is wasteful,
and real systems save the work of earlier steps. For a model this small it
doesn't matter.

Before calling `pick`, `generate` divides the scores by a number called
the *temperature*. We'll see what that does in a moment.

```aura fragment
def main() -> int32:
    device = Device.cpu()
    context = 64
    match own fs.read_to_string("input.txt"):
        case Err(error):
            print(f"cannot read input.txt: {error}")
            return 1
        case Ok(text):
            vocabulary = build_vocabulary(text)
            mut model = build(vocabulary.letters.len(), context, 64, 4, 2, device)
            args = sys.args()
            if args.len() > 0 and args[0] == "train":
                ids = vocabulary.encode(text)
                split = ids.len() * 9 // 10
                train(model, as_tensor(ids[:split], device), as_tensor(ids[split:], device), context, 1500)
                match save_model(model, "shakespeare.safetensors"):
                    case Ok(bytes):
                        print(f"saved {bytes} bytes")
                    case Err(message):
                        print(f"could not save: {message}")
                        return 1
                return 0

            match load_model(model, "shakespeare.safetensors", device):
                case Ok(count):
                    print(f"loaded {count} tensors")
                case Err(message):
                    print(f"could not load: {message}")
                    return 1
            tensor.set_grad_enabled(false)
            for temperature in [0.5, 1.0, 1.5]:
                mut rng = random.Rng(7)
                out = generate(model, vocabulary.encode("\n"), 240, context, temperature, rng, device)
                print(f"--- temperature {temperature}")
                print(vocabulary.decode(out))
    return 0
```

<span class="caption">Listing 24-6: A `main` that trains when asked and writes otherwise</span>

The model has to be built before it can be loaded: `build` makes a model
of the right shape with random weights, and `load_model` replaces them
with the trained ones. If the shapes in the file did not match the model,
`load_state_dict` would refuse, as we saw in Chapter 23.

Arguments for the program go after `--`, so `aura run --backend direct
src/main.au -- train` would train again. We already have the file from
the last section, so we run it without arguments. It starts from a single
line break and writes 240 characters, three times.

```console
$ aura run --backend direct src/main.au
loaded 38 tensors
--- temperature 0.5

Thou that or the king, and the down of your marry state.

PRICHARD:
I am the not a pring are thinks my distres
And part the commed warr'd hort endone.

Servengman:
I am the of the part as married the fair and face
And leaves and thou will n
--- temperature 1.0

Servy:
And thou did boser and a swore, you:
Ravoure.
My clove wistrear thee, me a line agand of as one;
My vit I will neppecrorce
To and bevire minire know'd on our catare;
Many good that lest whife, bleas; That
I come basior ma. Yorken my 
--- temperature 1.5

Saytxibal mputham:
Aeging Enas Ruyolbity?

CLARENCENThiranry itChroman's formal.

KING EDWICENTY VIAPUUERT: KucHereform mooe ungl ove let fwor chignoom-someed-lars?

Dicihend Go?
As con;--Firstroth oqueakilo,
Be fighn bacrist le'ssuchly lun
```

## What the model learned

Read the first sample. Ten minutes ago this model's output was 65 equally
likely characters. Now there are speaker names in capitals on their own
lines, each followed by a colon. Speeches begin with capital letters and
break into lines of about the right length. There is a blank line between
speeches. Most of the words are real, and most of the rest could be: they
are built from English syllables in an English order. Phrases like `I am
the` and `the king` appear. Nobody told the model about any of these
things. Each is a pattern that made the next character easier to predict.

It is also plainly not Shakespeare, or sense. The model sees 64
characters and has two blocks to think with. It has learned the texture
of the text and none of its meaning. The models that write coherent pages
are this program with more of everything: more blocks, wider vectors,
longer context, and far more text. The next chapter runs one of them.

## Temperature

The three samples differ only in the temperature, and they all draw the
same random numbers, so the differences are the temperature's doing.

`softmax` turns scores into chances, and what matters to it is the
difference between scores. Dividing all the scores by 0.5 doubles every
difference, so the best-scoring characters take even more of the chance
and the unlikely ones almost none. The model plays safe: the first sample
has the most real words, and also the most repetition, with `I am the`
twice. Dividing by 1.5 shrinks the differences and gives unlikely
characters a real chance. One odd character leads to text the model never
saw in training, where its predictions are worse, and the third sample
falls apart into noise. A temperature of 1.0 uses the model's chances as
they are.

At the limit of a very low temperature the model would always take the
single most likely character, which is what `argmax` would do. That sounds
like the best choice and usually isn't: text written that way tends to
fall into a loop and repeat itself. A little randomness is what lets
the model leave the loop. Most systems use a temperature somewhat below 1.

## Things to try

The program is yours to experiment with, and each of these is a change of
a line or two:

- Pass something other than a line break as the start, such as
  `vocabulary.encode("ROMEO:")`, and the model continues from it. Every
  character of the start must be one of the 65 in the vocabulary.
- Train for 3,000 steps where we used 1,500, or build the model with
  three blocks or a width of 96, and compare the held-out loss.
- Change the context to 32, then to 128, and see what a shorter or longer
  memory does to the samples.
- Use a different text. Any plain text file of a megabyte or so will do,
  and the model will imitate that one.

## Summary

A language model is trained to predict the next token of a text, and
writes by predicting one token, appending it, and predicting again. To
build one we turned characters into numbers with a vocabulary, cut the
text into windows whose targets are the same windows moved by one, and
measured a baseline with a table of which character follows which.

A transformer improves on the table by letting every position take
information from the positions before it. Attention does that with
queries, keys, and values: scores from comparing queries with keys, a mask
so that no position sees ahead, `softmax` to turn scores into weights, and
a weighted average of the values. A block is attention followed by a
small network, each added to its input, with a layer norm before each.
The model is two embeddings, a stack of blocks, and a layer that turns the
last vectors into scores. We trained it with the loop from Chapter 22,
saved it, loaded it, and controlled how boldly it writes with a
temperature.

In the next chapter we take the same parts and assemble them to the
measurements of GPT-2, load the weights that OpenAI trained, and write
with a real model.
