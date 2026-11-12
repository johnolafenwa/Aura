# Regular expressions over generated log lines.
#
# The same work as regex_scan.au, on the standard library's `re`.
import re
import sys


class Rng:
    def __init__(self, state):
        self.state = state

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def main():
    if len(sys.argv) != 2:
        print("usage: regex_scan <lines>")
        return 2
    n = int(sys.argv[1])
    level_re = re.compile(r"\b(?:ERROR|WARN)\b")
    pair_re = re.compile(r"(?P<key>[a-z]+)=(\d+)")
    number_re = re.compile(r"\d+")
    address_re = re.compile(r"([a-z]+)@([a-z]+)\.example")
    separator_re = re.compile(r"[,;]\s*")

    rng = Rng(97)
    levels = ["INFO", "DEBUG", "WARN", "INFO", "ERROR", "INFO", "TRACE", "INFO"]
    names = ["ada", "grace", "linus", "barbara", "ken", "margaret", "dennis", "frances"]
    hosts = ["mail", "corp", "lab", "dev"]
    flagged = pairs = pair_sum = numbers = spans = rewritten_len = piece_count = checksum = 0
    for i in range(n):
        level = levels[rng.next() % 8]
        name = names[rng.next() % 8]
        host = hosts[rng.next() % 4]
        line = (
            f"{i} {level} user={rng.next() % 5000} took={rng.next() % 900}ms "
            + f"from {name}@{host}.example; tags a{rng.next() % 10}, b{rng.next() % 10};c"
        )
        if level_re.search(line):
            flagged += 1

        groups = pair_re.search(line)
        if groups is not None:
            pairs += 1
            pair_sum += int(groups.group(2))
            checksum = (checksum * 31 + len(groups.group("key"))) % 1000000007

        for found in number_re.finditer(line):
            numbers += 1
            spans += found.end() - found.start()
            checksum = (checksum * 31 + found.start()) % 1000000007

        rewritten = address_re.sub(r"\2/\1", line)
        rewritten_len += len(rewritten)

        pieces = separator_re.split(line)
        piece_count += len(pieces)
        checksum = (checksum * 31 + len(pieces[-1])) % 1000000007

    print(
        f"regex_scan lines={n} flagged={flagged} pairs={pairs} "
        f"pair_sum={pair_sum} numbers={numbers} spans={spans} "
        f"rewritten={rewritten_len} pieces={piece_count} checksum={checksum}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
