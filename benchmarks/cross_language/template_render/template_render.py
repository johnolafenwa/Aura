"""Web rendering: a Mustache-style template engine producing HTML pages.

Compiles one page template with escaped variables ({{name}}), raw variables
({{&name}}), and a repeated section ({{#rows}}...{{/rows}}) into segments, then
renders N pages, each from its own context with 20 table rows. Text that goes
through {{name}} is HTML-escaped. Reports the pages, rows, bytes rendered, the
escapes written, and a checksum of every page's length.
"""
import sys

TEMPLATE = (
    "<!doctype html>\n<html><head><title>{{title}}</title></head>\n"
    "<body><h1>{{title}}</h1>\n<p class=\"greeting\">Welcome back, {{user}}!</p>\n"
    "<table>\n{{#rows}}<tr><td>{{name}}</td><td class=\"price\">${{price}}</td>"
    "<td>{{note}}</td><td>{{user}}</td></tr>\n{{/rows}}</table>\n"
    "<footer>{{&footer}}</footer>\n</body></html>\n"
)
LITERAL = 0
ESCAPED = 1
RAW = 2
SECTION = 3
END = 4
SYLLABLES = ["ka", "lo", "mi", "ra", "to", "ne", "su", "vi"]
NOTES = ["fresh & local", "<limited>", "\"best\" seller", "it's new", "plain", "a < b > c"]
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def compile_template(source):
    kinds = []
    texts = []
    parts = source.split("{{")
    kinds.append(LITERAL)
    texts.append(parts[0])
    for part in parts[1:]:
        tag, literal = part.split("}}", 1)
        if tag.startswith("#"):
            kinds.append(SECTION)
            texts.append(tag[1:])
        elif tag.startswith("/"):
            kinds.append(END)
            texts.append(tag[1:])
        elif tag.startswith("&"):
            kinds.append(RAW)
            texts.append(tag[1:])
        else:
            kinds.append(ESCAPED)
            texts.append(tag)
        kinds.append(LITERAL)
        texts.append(literal)
    return kinds, texts


def escape(text):
    return (
        text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        .replace("\"", "&quot;").replace("'", "&#39;")
    )


def lookup(name, row, context):
    if name in row:
        return row[name]
    return context[name]


def render_range(kinds, texts, start, end, row, context, rows, out):
    i = start
    while i < end:
        kind = kinds[i]
        if kind == LITERAL:
            out.append(texts[i])
        elif kind == ESCAPED:
            out.append(escape(lookup(texts[i], row, context)))
        elif kind == RAW:
            out.append(lookup(texts[i], row, context))
        elif kind == SECTION:
            close = i + 1
            while kinds[close] != END:
                close += 1
            for item in rows:
                render_range(kinds, texts, i + 1, close, item, context, rows, out)
            i = close
        i += 1


def make_page(rng, page):
    context = {
        "title": "Catalog page " + str(page) + " <" + SYLLABLES[page % 8] + ">",
        "user": "user<" + str(rng.next() % 1000) + ">&co",
        "footer": "<small>Generated page " + str(page) + "</small>",
    }
    rows = []
    for _ in range(20):
        r = rng.next()
        cents = r % 100000
        rows.append({
            "name": SYLLABLES[r % 8] + SYLLABLES[(r // 8) % 8] + SYLLABLES[(r // 64) % 8],
            "price": str(cents // 100) + "." + f"{cents % 100:02d}",
            "note": NOTES[(r // 512) % 6],
        })
    return context, rows


def main():
    n = int(sys.argv[1])
    rng = Rng(61)
    kinds, texts = compile_template(TEMPLATE)
    total_bytes = 0
    escapes = 0
    checksum = 0
    for page in range(n):
        context, rows = make_page(rng, page)
        out = []
        render_range(kinds, texts, 0, len(kinds), {}, context, rows, out)
        html = "".join(out)
        total_bytes += len(html.encode())
        escapes += html.count("&")
        checksum = (checksum * 31 + len(html)) % MOD
    print(
        f"template_render pages={n} segments={len(kinds)} rows={n * 20} bytes={total_bytes} "
        f"escapes={escapes} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
