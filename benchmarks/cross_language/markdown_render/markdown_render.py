"""Documentation tools: render Markdown to HTML.

Generates a Markdown document of N blocks (headings, paragraphs, bulleted and
numbered lists, fenced code, and quotes) whose text uses **bold**, *italic*,
`code`, and [links](url), then renders it to HTML in one pass over the lines.
Inline markup is scanned byte by byte, text is HTML-escaped, and the output is
built in a byte buffer. Reports the input and output sizes, the elements
written, and a checksum of the HTML.
"""
import sys

WORDS = ["aura", "fast", "safe", "compiler", "runtime", "task", "queue", "list", "native", "build", "docs", "test"]
NONE = 0
PARAGRAPH = 1
BULLETS = 2
NUMBERS = 3


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def inline_text(rng, count):
    parts = []
    for _ in range(count):
        r = rng.next()
        word = WORDS[r % 12]
        style = (r // 12) % 10
        if style == 0:
            parts.append("**" + word + "**")
        elif style == 1:
            parts.append("*" + word + "*")
        elif style == 2:
            parts.append("`" + word + "<T>`")
        elif style == 3:
            parts.append("[" + word + "](https://example.com/" + word + "?a=1&b=2)")
        elif style == 4:
            parts.append(word + " & co")
        else:
            parts.append(word)
    return " ".join(parts)


def generate(rng, blocks):
    lines = []
    for _ in range(blocks):
        r = rng.next()
        kind = r % 6
        if kind == 0:
            lines.append("#" * (1 + (r // 6) % 3) + " " + inline_text(rng, 3 + (r // 18) % 4))
        elif kind == 1:
            for _ in range(1 + (r // 6) % 3):
                lines.append(inline_text(rng, 6 + (r // 18) % 6))
        elif kind == 2:
            for _ in range(2 + (r // 6) % 4):
                lines.append("- " + inline_text(rng, 4))
        elif kind == 3:
            for i in range(2 + (r // 6) % 4):
                lines.append(str(i + 1) + ". " + inline_text(rng, 4))
        elif kind == 4:
            lines.append("```")
            for i in range(1 + (r // 6) % 4):
                lines.append("if a < b && c > " + str(i) + ": return \"x\"")
            lines.append("```")
        else:
            lines.append("> " + inline_text(rng, 5))
        lines.append("")
    return "\n".join(lines)


def escape_into(out, data, start, end):
    for i in range(start, end):
        c = data[i]
        if c == 38:
            out.extend(b"&amp;")
        elif c == 60:
            out.extend(b"&lt;")
        elif c == 62:
            out.extend(b"&gt;")
        elif c == 34:
            out.extend(b"&quot;")
        else:
            out.append(c)


def find(data, start, end, c):
    for i in range(start, end):
        if data[i] == c:
            return i
    return -1


def render_inline(out, data, start, end):
    i = start
    while i < end:
        c = data[i]
        if c == 42 and i + 1 < end and data[i + 1] == 42:
            close = find(data, i + 2, end, 42)
            if close >= 0 and close + 1 < end and data[close + 1] == 42:
                out.extend(b"<strong>")
                escape_into(out, data, i + 2, close)
                out.extend(b"</strong>")
                i = close + 2
                continue
        elif c == 42:
            close = find(data, i + 1, end, 42)
            if close >= 0:
                out.extend(b"<em>")
                escape_into(out, data, i + 1, close)
                out.extend(b"</em>")
                i = close + 1
                continue
        elif c == 96:
            close = find(data, i + 1, end, 96)
            if close >= 0:
                out.extend(b"<code>")
                escape_into(out, data, i + 1, close)
                out.extend(b"</code>")
                i = close + 1
                continue
        elif c == 91:
            label_end = find(data, i + 1, end, 93)
            if label_end >= 0 and label_end + 1 < end and data[label_end + 1] == 40:
                url_end = find(data, label_end + 2, end, 41)
                if url_end >= 0:
                    out.extend(b"<a href=\"")
                    escape_into(out, data, label_end + 2, url_end)
                    out.extend(b"\">")
                    escape_into(out, data, i + 1, label_end)
                    out.extend(b"</a>")
                    i = url_end + 1
                    continue
        escape_into(out, data, i, i + 1)
        i += 1


def close_block(out, state):
    if state == PARAGRAPH:
        out.extend(b"</p>\n")
    elif state == BULLETS:
        out.extend(b"</ul>\n")
    elif state == NUMBERS:
        out.extend(b"</ol>\n")
    return NONE


def render(text):
    out = bytearray()
    counts = [0] * 6
    state = NONE
    in_code = False
    for line in text.split("\n"):
        data = line.encode()
        n = len(data)
        if n >= 3 and data[0] == 96 and data[1] == 96 and data[2] == 96:
            if in_code:
                out.extend(b"</code></pre>\n")
                in_code = False
            else:
                state = close_block(out, state)
                out.extend(b"<pre><code>")
                in_code = True
                counts[4] += 1
            continue
        if in_code:
            escape_into(out, data, 0, n)
            out.append(10)
            continue
        if n == 0:
            state = close_block(out, state)
            continue
        if data[0] == 35:
            state = close_block(out, state)
            level = 0
            while level < n and data[level] == 35:
                level += 1
            out.extend(b"<h")
            out.append(48 + level)
            out.append(62)
            render_inline(out, data, level + 1, n)
            out.extend(b"</h")
            out.append(48 + level)
            out.extend(b">\n")
            counts[0] += 1
        elif n >= 2 and data[0] == 45 and data[1] == 32:
            if state != BULLETS:
                state = close_block(out, state)
                out.extend(b"<ul>\n")
                state = BULLETS
            out.extend(b"<li>")
            render_inline(out, data, 2, n)
            out.extend(b"</li>\n")
            counts[2] += 1
        elif 48 <= data[0] <= 57:
            dot = find(data, 0, n, 46)
            if state != NUMBERS:
                state = close_block(out, state)
                out.extend(b"<ol>\n")
                state = NUMBERS
            out.extend(b"<li>")
            render_inline(out, data, dot + 2, n)
            out.extend(b"</li>\n")
            counts[3] += 1
        elif data[0] == 62:
            state = close_block(out, state)
            out.extend(b"<blockquote>")
            render_inline(out, data, 2, n)
            out.extend(b"</blockquote>\n")
            counts[5] += 1
        else:
            if state != PARAGRAPH:
                state = close_block(out, state)
                out.extend(b"<p>")
                state = PARAGRAPH
                counts[1] += 1
            else:
                out.append(10)
            render_inline(out, data, 0, n)
    close_block(out, state)
    return out, counts


def main():
    n = int(sys.argv[1])
    rng = Rng(83)
    text = generate(rng, n)
    html, counts = render(text)
    checksum = 0
    for c in html:
        checksum = (checksum * 31 + c) % 1000000007
    print(
        f"markdown_render blocks={n} markdown_bytes={len(text.encode())} html_bytes={len(html)} "
        f"headings={counts[0]} paragraphs={counts[1]} items={counts[2] + counts[3]} code={counts[4]} "
        f"quotes={counts[5]} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
