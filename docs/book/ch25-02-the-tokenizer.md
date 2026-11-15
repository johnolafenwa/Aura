# The tokenizer

Our Shakespeare model read one character at a time. That kept the
vocabulary tiny, at a price: the model spent much of its small capacity
learning to spell, and 64 tokens of context was only a dozen words.

At the other extreme, every word could be a token. Then context goes much
further, but the vocabulary is enormous and never complete. A new name, a
typo, or a word in another language would have no token at all.

GPT-2 uses a scheme between the two, called *byte pair encoding*. Common
words are one token. Rarer words are cut into a few pieces that are each
common, like `token` and `izers`. And in the worst case any text at all
can be written as single bytes, so nothing is ever outside the vocabulary.
There are 50,257 tokens.

## How the vocabulary was made

The rule is simple, and seeing how the vocabulary was built explains how
to use it.

Start with 256 tokens, one for each possible byte. Take a large body of
text and find the pair of neighboring tokens that occurs most often, say
`t` followed by `h`. Make that pair a new token, `th`, and replace it
everywhere. Now find the most common pair again, which may include the
new token: perhaps `th` followed by `e`. Repeat 50,000 times. Frequent
words get assembled into single tokens early, and rare words are left as
a few pieces.

OpenAI did that once and published the result as the two small files we
downloaded. `merges.txt` lists the 50,000 merges in the order they were
made, one pair per line. `vocab.json` gives the number of every token.

To tokenize new text we replay the merges. Start with the bytes of a
word. Of all the neighboring pairs in it, find the one that was merged
earliest in `merges.txt`, and join it. Repeat until no neighboring pair is
in the list. What is left are the tokens.

## Two details of GPT-2's files

First, text is cut into words before any merging, and merges never cross
from one word into the next. A word takes the space before it along, so
` model` with its leading space is a different token from `model` at the
start of a line. A regular expression does the cutting. It matches, in
this order: a few English contractions such as `'s` and `'ll`, a run of
letters with an optional space in front, a run of digits the same way, a
run of anything else that is not whitespace, and a run of whitespace.

Second, the files can't hold raw bytes, because many bytes are not
printable characters, and the space is one of the awkward ones. So GPT-2's
files write each of the 256 bytes as a printable character. Bytes that
print well are written as themselves, and the others are moved up to
unused characters starting at number 256. The space, byte 32, becomes `Ġ`.
You will see `Ġ` at the front of most tokens in the files. It only means
"this token begins with a space".

## The code

*Filename: src/main.au*

```aura
import fs
import json
import regex

class Tokenizer:
    ids: dict[str, int64]
    texts: list[str]
    ranks: dict[str, int64]
    letters: list[str]
    bytes: dict[str, int64]
    pattern: regex.Regex

    def split(self, text: str) -> list[str]:
        # Cuts text into words with their leading space, numbers,
        # punctuation, and runs of whitespace.
        mut pieces: list[str] = []
        mut rest = text.clone()
        while rest.len() > 0:
            match self.pattern.find(rest):
                case None:
                    break
                case regex.Match as found:
                    mut piece = found.text.clone()
                    if piece.len() > 1 and piece.len() < rest.len() and piece.trim() == "":
                        # Leave the last space of a run for the word after it.
                        piece = piece[:piece.len() - 1]
                    rest = rest[piece.len():]
                    pieces.append(piece)
        return pieces

    def merge(self, piece: str) -> list[str]:
        # Starts from single bytes and joins the best-ranked pair until
        # no pair of neighbors is in the table.
        mut parts: list[str] = []
        for byte in piece.to_bytes():
            parts.append(self.letters[byte as int64].clone())
        while parts.len() > 1:
            mut best = -1
            mut best_rank = self.ranks.len()
            for at in range(parts.len() - 1):
                pair = parts[at] + " " + parts[at + 1]
                if pair in self.ranks:
                    rank = self.ranks[pair]
                    if rank < best_rank:
                        best_rank = rank
                        best = at
            if best < 0:
                break
            left = parts[best].clone()
            right = parts[best + 1].clone()
            mut merged: list[str] = []
            mut at = 0
            while at < parts.len():
                if at + 1 < parts.len() and parts[at] == left and parts[at + 1] == right:
                    merged.append(left + right)
                    at += 2
                else:
                    merged.append(parts[at].clone())
                    at += 1
            parts = merged
        return parts

    def encode(self, text: str) -> list[int64]:
        mut out: list[int64] = []
        for piece in self.split(text):
            for part in self.merge(piece):
                out.append(self.ids[part])
        return out

    def decode(self, tokens: list[int64]) -> str:
        mut raw: list[uint8] = []
        for token in tokens:
            for letter in self.texts[token].chars():
                raw.append(self.bytes[letter] as uint8)
        match own str.from_bytes(raw):
            case Ok(text):
                return text
            case Err(error):
                return "?"

def byte_letters() -> list[str]:
    # GPT-2 writes each of the 256 byte values as one printable character.
    mut letters: list[str] = []
    mut spare = 256
    for byte in range(256):
        printable = (byte >= 33 and byte <= 126) or (byte >= 161 and byte <= 172) or byte >= 174
        if printable:
            letters.append(chr(byte))
        else:
            letters.append(chr(spare))
            spare += 1
    return letters

def load_tokenizer(vocab_path: str, merges_path: str) -> Result[Tokenizer, str]:
    mut ids = dict[str, int64]()
    match own fs.read_to_string(vocab_path):
        case Err(error):
            return Err(f"cannot read {vocab_path}: {error}")
        case Ok(text):
            match own json.parse(text):
                case Err(error):
                    return Err(f"{vocab_path} is not JSON")
                case Ok(value):
                    match own json.into_object(value):
                        case None:
                            return Err(f"{vocab_path} is not a JSON object")
                        case dict[str, json.Value] as table:
                            for name in table.keys():
                                view entry = table[name]
                                match json.as_int(entry):
                                    case None:
                                        return Err(f"{vocab_path} has a token without a number")
                                    case int64 as id:
                                        ids[name.clone()] = id
    mut texts = [""] * ids.len()
    for text in ids.keys():
        texts[ids[text]] = text.clone()
    mut ranks = dict[str, int64]()
    match own fs.read_to_string(merges_path):
        case Err(error):
            return Err(f"cannot read {merges_path}: {error}")
        case Ok(text):
            for line in text.split("\n"):
                if line == "" or line.starts_with("#version"):
                    continue
                rank = ranks.len()
                ranks[line.clone()] = rank
    letters = byte_letters()
    mut bytes = dict[str, int64]()
    for byte in range(256):
        bytes[letters[byte].clone()] = byte
    match own regex.compile("'s|'t|'re|'ve|'m|'ll|'d| ?\\p{L}+| ?\\p{N}+| ?[^\\s\\p{L}\\p{N}]+|\\s+"):
        case Err(error):
            return Err("the pattern did not compile")
        case Ok(pattern):
            return Ok(Tokenizer(ids=ids, texts=texts, ranks=ranks, letters=letters, bytes=bytes, pattern=pattern))

def main() -> int32:
    match own load_tokenizer("vocab.json", "merges.txt"):
        case Err(message):
            print(message)
            return 1
        case Ok(tokenizer):
            print(f"{tokenizer.texts.len()} tokens, {tokenizer.ranks.len()} merges")
            tokens = tokenizer.encode("Hello, I'm a language model,")
            print(tokens)
            for token in tokens:
                print(f"{token}: {tokenizer.texts[token]}")
            print(tokenizer.decode(tokens))
            print(tokenizer.merge(" tokenizers"))
            print(tokenizer.encode("naïve café"))
            print(tokenizer.decode(tokenizer.encode("naïve café")))
    return 0
```

<span class="caption">Listing 25-2: A byte pair tokenizer that reads GPT-2's vocabulary files</span>

```console
$ aura run --backend direct src/main.au
50257 tokens, 50000 merges
[15496, 11, 314, 1101, 257, 3303, 2746, 11]
15496: Hello
11: ,
314: ĠI
1101: 'm
257: Ġa
3303: Ġlanguage
2746: Ġmodel
11: ,
Hello, I'm a language model,
[Ġtoken, izers]
[2616, 38776, 40304]
naïve café
```

`Tokenizer` holds five tables. `ids` maps a token's text to its number and
`texts` is the same table the other way around. `ranks` maps a pair,
written as in `merges.txt` with a space between the halves, to its line
number in that file. `letters` and `bytes` translate between the 256 bytes
and the printable characters that stand for them.

`split` cuts text into words. It asks the pattern for the first match in
the text that is left, takes it, and goes on from there. There is one
wrinkle. When several spaces come before a word, the reference tokenizer
gives all but the last to a whitespace token and leaves the last attached
to the word. It does that with a feature of regular expressions called
lookahead, which Aura's `regex` module doesn't have, so we do it with an
`if`: a whitespace match of more than one character that is not at the end
of the text gives back its last character.

`merge` is the heart of the scheme, and it is the procedure we described.
It turns a word into its bytes, written as their printable characters, and
then loops. Each time around it looks at every neighboring pair, finds the
one with the lowest rank, and joins every occurrence of that pair. When no
pair has a rank, it stops.

`encode` is then two lines: split, merge each word, and look up each part
in `ids`. `decode` goes backward. It writes out the text of every token,
turns the printable characters back into bytes, and reads the bytes as
UTF-8.

`load_tokenizer` reads the two files. The vocabulary is a JSON object from
token text to number, which we walk with the `json` module of [Chapter
16](/book/ch16-04-json). The rank of a merge is the number of
merges read before it. The pattern is the regular expression described
above.

## Reading the output

Our test sentence, `Hello, I'm a language model,`, becomes eight tokens.
Every word in it is common enough to be a single token, and you can see
the leading spaces as `Ġ`. The contraction `'m` is its own token, cut
off by the first branch of the pattern. Decoding the eight numbers gives
back the sentence exactly.

A rarer word is cut into pieces: ` tokenizers` becomes ` token` and
`izers`. The model has never seen that word as a unit, but it knows
both halves well.

The last two lines show the byte fallback. `naïve café` has two letters
outside ASCII. In UTF-8 each is two bytes, the merges assembled what they
could, and three tokens came out. Decoding returns the original text,
accents included. Any string at all survives the round trip, in any
language, because at the bottom there is always a token for every byte.

It is worth being sure the tokenizer is right. A model fed tokens cut
differently from its training data does not fail. It quietly writes worse
text, and that is hard to notice. We checked this tokenizer against the
reference implementation on a few hundred texts, including unusual
spacing, punctuation, and several scripts, and the token numbers were
identical on all of them.
