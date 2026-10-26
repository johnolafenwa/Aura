"""Text processing: tokenize a generated corpus and count words and bigrams.

Builds a corpus of N words with mixed case and punctuation, then splits it
into lines and tokens, normalizes each token, counts words and adjacent word
pairs, and ranks the ten most frequent words.
"""
import sys

SYLLABLES = ["ka", "lo", "mi", "ra", "zu", "te", "no", "bi", "sa", "de", "gu", "pe"]
VOCABULARY = 2000


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def build_corpus(rng, n):
    vocabulary = []
    for _ in range(VOCABULARY):
        r = rng.next()
        parts = []
        for k in range(1 + r % 4):
            parts.append(SYLLABLES[(r // (k * 12 + 4)) % 12])
        vocabulary.append("".join(parts))
    lines = []
    line = []
    for i in range(n):
        a = rng.next() % VOCABULARY
        b = rng.next() % VOCABULARY
        r = rng.next()
        word = vocabulary[a * b // VOCABULARY]
        if r % 7 == 0:
            word = word.upper()
        if r % 11 == 0:
            word += ","
        elif r % 13 == 0:
            word += "."
        line.append(word)
        if i % 12 == 11:
            lines.append(" ".join(line))
            line = []
    if line:
        lines.append(" ".join(line))
    return "\n".join(lines) + "\n"


def main():
    n = int(sys.argv[1])
    corpus = build_corpus(Rng(3), n)
    counts = {}
    bigrams = {}
    tokens = 0
    for line in corpus.splitlines():
        previous = None
        for token in line.split():
            word = token.lower()
            if word.endswith(",") or word.endswith("."):
                word = word[:-1]
            tokens += 1
            counts[word] = counts.get(word, 0) + 1
            if previous is not None:
                pair = previous + " " + word
                bigrams[pair] = bigrams.get(pair, 0) + 1
            previous = word
    ranked = list(counts.items())
    ranked.sort(key=lambda entry: -entry[1])
    top = " ".join(f"{word}:{count}" for word, count in ranked[:10])
    weight = sum(count * len(word) for word, count in counts.items())
    print(
        f"word_freq tokens={tokens} bytes={len(corpus)} distinct={len(counts)} "
        f"bigrams={len(bigrams)} weight={weight} top={top}"
    )


if __name__ == "__main__":
    main()
