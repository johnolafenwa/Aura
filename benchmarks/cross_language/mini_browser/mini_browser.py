"""Web browser: fetch pages over HTTP, parse HTML and CSS, lay out text, follow links.

A server task generates a site of N HTML pages, each with a style sheet,
headings, paragraphs, lists, inline links, and hidden ad blocks. The browser
crawls the site breadth first from /page/0, one request per connection. For
each page it tokenizes the HTML into a DOM tree, parses the <style> sheet,
computes each element's style with tag and class selectors and specificity,
and lays the visible text out into lines with word wrapping, indents, margins,
and numbered link markers, like a text-mode browser. Links to missing pages get
404 replies. Reports crawl counts and a checksum over every rendered line.
"""
import asyncio
import sys

WORDS = [
    "alpha", "bravo", "cache", "delta", "engine", "fabric", "gamma", "harbor",
    "index", "jolly", "kernel", "lambda", "matrix", "nectar", "orbit", "packet",
    "quartz", "router", "socket", "tensor", "update", "vector", "widget", "xenon",
    "yield", "zephyr", "browser", "layout", "render", "stream", "token", "cursor",
]
STYLE = (
    "body { width: 72 }\n"
    "h1 { transform: upper; margin: 1 }\n"
    "h2 { margin: 1 }\n"
    "p { indent: 2 }\n"
    "ul { margin: 1 }\n"
    "li { indent: 4 }\n"
    ".note { indent: 6; width: 56 }\n"
    ".wide { width: 96 }\n"
    ".hidden, .ad { display: none }\n"
    "a.ext { transform: upper }\n"
)
NOT_FOUND = "<html><body><h1>Not found</h1></body></html>"
HIDDEN_TAGS = {"head", "title", "style", "script"}
BLOCK_TAGS = {"html", "body", "h1", "h2", "p", "div", "ul", "li"}
VOID_TAGS = {"br", "img", "meta", "hr"}
PROPERTIES = ["width", "indent", "margin", "transform", "display"]


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def words(rng, count):
    out = []
    for _ in range(count):
        out.append(WORDS[rng.next() % 32])
    return " ".join(out)


def inline_text(rng, total, count):
    parts = []
    for _ in range(count):
        r = rng.next()
        kind = r % 20
        word = WORDS[(r // 20) % 32]
        if kind == 0:
            target = (r // 640) % (total + total // 10 + 1)
            parts.append(f'<a href="/page/{target}">{word} {WORDS[(r // 7) % 32]}</a>')
        elif kind == 1:
            parts.append(f'<a class="ext" href="http://example.com/{word}">{word}</a>')
        elif kind == 2:
            parts.append(f"<b>{word}</b>")
        elif kind == 3:
            parts.append(f"<em>{word}</em>")
        elif kind == 4:
            parts.append(f"{word} &amp; co")
        elif kind == 5:
            parts.append(f"&lt;{word}&gt;")
        else:
            parts.append(word)
    return " ".join(parts)


def page_html(page, total):
    rng = Rng((page + 1) * 7919)
    out = [f"<!doctype html>\n<html><head><title>Page {page}</title>\n<style>\n{STYLE}</style></head>\n<body>\n"]
    out.append(f"<h1>Page {page}: {words(rng, 3)}</h1>\n")
    sections = 3 + rng.next() % 4
    for _ in range(sections):
        count = 2 + rng.next() % 3
        out.append(f"<h2>{words(rng, count)}</h2>\n")
        paragraphs = 1 + rng.next() % 3
        for _ in range(paragraphs):
            r = rng.next()
            classes = ["", ' class="note"', ' class="wide"', ' class="note wide"'][r % 4]
            count = 20 + (r // 4) % 40
            out.append(f"<p{classes}>{inline_text(rng, total, count)}</p>\n")
        r = rng.next()
        if r % 3 == 0:
            items = 2 + (r // 3) % 4
            out.append("<ul>\n")
            for _ in range(items):
                count = 3 + rng.next() % 6
                out.append(f"<li>{inline_text(rng, total, count)}</li>\n")
            out.append("</ul>\n")
        if r % 5 == 1:
            out.append('<div class="ad">Buy <b>now</b> &amp; save</div>\n')
        if r % 7 == 2:
            out.append("<!-- generated section -->\n<p>see<br>also</p>\n")
    out.append("</body></html>\n")
    return "".join(out)


class Node:
    def __init__(self, tag, classes, href, text):
        self.tag = tag
        self.classes = classes
        self.href = href
        self.text = text
        self.children = []


def add_child(nodes, parent, node):
    nodes.append(node)
    nodes[parent].children.append(len(nodes) - 1)
    return len(nodes) - 1


def decode_entities(text):
    if "&" not in text:
        return text
    return text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", '"').replace("&amp;", "&")


def parse_tag(inner):
    name, _, rest = inner.partition(" ")
    classes = []
    href = ""
    parts = rest.split('"')
    for k in range(0, len(parts) - 1, 2):
        attribute = parts[k].strip().removesuffix("=").strip().lower()
        if attribute == "class":
            classes = parts[k + 1].split()
        elif attribute == "href":
            href = parts[k + 1]
    return name.lower(), classes, href


def parse_html(html):
    nodes = [Node("#document", [], "", "")]
    stack = [0]
    i = 0
    n = len(html)
    while i < n:
        if html[i] != "<":
            end = html.find("<", i)
            if end < 0:
                end = n
            text = html[i:end]
            i = end
            if text.strip() != "":
                add_child(nodes, stack[-1], Node("", [], "", decode_entities(text)))
            continue
        if html.startswith("<!--", i):
            end = html.find("-->", i + 4)
            i = n if end < 0 else end + 3
            continue
        end = html.find(">", i)
        if end < 0:
            break
        inner = html[i + 1:end]
        i = end + 1
        if inner.startswith("!"):
            continue
        if inner.startswith("/"):
            name = inner[1:].strip().lower()
            for depth in range(len(stack) - 1, 0, -1):
                if nodes[stack[depth]].tag == name:
                    del stack[depth:]
                    break
            continue
        tag, classes, href = parse_tag(inner)
        index = add_child(nodes, stack[-1], Node(tag, classes, href, ""))
        if tag == "style" or tag == "script":
            end = html.find("</" + tag, i)
            if end < 0:
                end = n
            add_child(nodes, index, Node("", [], "", html[i:end]))
            i = end
            stack.append(index)
        elif tag not in VOID_TAGS:
            stack.append(index)
    return nodes


class Rule:
    def __init__(self, tag, class_name, specificity, prop, value):
        self.tag = tag
        self.class_name = class_name
        self.specificity = specificity
        self.prop = prop
        self.value = value


def property_value(prop, text):
    if prop == 3:
        return 1 if text == "upper" else 0
    if prop == 4:
        return 0 if text == "none" else 1 if text == "block" else 2
    return int(text)


def parse_css(text):
    rules = []
    for block in text.split("}"):
        if "{" not in block:
            continue
        selectors, body = block.split("{", 1)
        declarations = []
        for declaration in body.split(";"):
            if ":" not in declaration:
                continue
            name, value = declaration.split(":", 1)
            name = name.strip()
            if name in PROPERTIES:
                prop = PROPERTIES.index(name)
                declarations.append((prop, property_value(prop, value.strip())))
        for selector in selectors.split(","):
            tag, _, class_name = selector.strip().partition(".")
            specificity = (1 if tag else 0) + (10 if class_name else 0)
            for prop, value in declarations:
                rules.append(Rule(tag, class_name, specificity, prop, value))
    return rules


class Style:
    def __init__(self, width, indent, margin, upper, display):
        self.width = width
        self.indent = indent
        self.margin = margin
        self.upper = upper
        self.display = display


def compute_style(node, parent, rules):
    values = [parent.width, parent.indent, 0, parent.upper, 2]
    if node.tag in HIDDEN_TAGS:
        values[4] = 0
    elif node.tag in BLOCK_TAGS:
        values[4] = 1
    best = [-1, -1, -1, -1, -1]
    for rule in rules:
        if rule.tag and rule.tag != node.tag:
            continue
        if rule.class_name and rule.class_name not in node.classes:
            continue
        if rule.specificity >= best[rule.prop]:
            best[rule.prop] = rule.specificity
            values[rule.prop] = rule.value
    return Style(values[0], values[1], values[2], values[3], values[4])


class Layout:
    def __init__(self):
        self.lines = []
        self.words = []
        self.line_len = 0
        self.width = 80
        self.indent = 0
        self.links = []
        self.word_count = 0

    def flush(self):
        if self.words:
            self.lines.append(" " * self.indent + " ".join(self.words))
            self.words = []
            self.line_len = 0

    def add_word(self, word):
        available = max(self.width - self.indent, 1)
        if self.words and self.line_len + 1 + len(word) > available:
            self.flush()
        if self.words:
            self.line_len += 1
        self.line_len += len(word)
        self.words.append(word)
        self.word_count += 1


def render(nodes, index, parent, rules, layout):
    node = nodes[index]
    if node.tag == "":
        for word in node.text.split():
            layout.add_word(word.upper() if parent.upper else word)
        return
    style = compute_style(node, parent, rules)
    if style.display == 0:
        return
    if node.tag == "br":
        layout.flush()
        return
    block = style.display == 1
    saved_width = layout.width
    saved_indent = layout.indent
    if block:
        layout.flush()
        for _ in range(style.margin):
            layout.lines.append("")
        layout.width = style.width
        layout.indent = style.indent
    for child in node.children:
        render(nodes, child, style, rules, layout)
    if node.tag == "a" and node.href != "":
        layout.links.append(node.href)
        layout.add_word(f"[{len(layout.links)}]")
    if block:
        layout.flush()
        layout.width = saved_width
        layout.indent = saved_indent


class Page:
    def __init__(self, nodes, title_bytes, layout):
        self.nodes = nodes
        self.title_bytes = title_bytes
        self.layout = layout


def browse(html):
    nodes = parse_html(html)
    sheet = []
    title_bytes = 0
    for node in nodes:
        if node.tag == "style":
            for child in node.children:
                sheet.append(nodes[child].text)
        elif node.tag == "title":
            for child in node.children:
                title_bytes += len(nodes[child].text)
    rules = parse_css("".join(sheet))
    layout = Layout()
    render(nodes, 0, Style(80, 0, 0, 0, 1), rules, layout)
    layout.flush()
    return Page(len(nodes), title_bytes, layout)


async def read_message(reader):
    start = (await reader.readline()).decode().rstrip("\r\n")
    headers = {}
    while True:
        line = (await reader.readline()).decode().rstrip("\r\n")
        if line == "":
            break
        name, value = line.split(":", 1)
        headers[name.strip().lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    body = (await reader.readexactly(length)).decode() if length else ""
    return start, body


async def serve(port_ready, total):
    served = [0]
    done = asyncio.get_running_loop().create_future()

    async def handle(reader, writer):
        start, _ = await read_message(reader)
        path = start.split(" ")[1]
        status = 200
        if path == "/quit":
            payload = "bye"
        else:
            page = int(path.removeprefix("/page/"))
            served[0] += 1
            if page < total:
                payload = page_html(page, total)
            else:
                status = 404
                payload = NOT_FOUND
        data = payload.encode()
        writer.write(
            f"HTTP/1.1 {status} OK\r\nContent-Type: text/html\r\n"
            f"Content-Length: {len(data)}\r\nConnection: close\r\n\r\n".encode() + data
        )
        await writer.drain()
        writer.close()
        await writer.wait_closed()
        if path == "/quit":
            done.set_result(None)

    server = await asyncio.start_server(handle, "127.0.0.1", 0)
    port_ready.set_result(server.sockets[0].getsockname()[1])
    await done
    server.close()
    await server.wait_closed()
    return served[0]


async def fetch(port, path):
    reader, writer = await asyncio.open_connection("127.0.0.1", port)
    writer.write(f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".encode())
    await writer.drain()
    start, body = await read_message(reader)
    writer.close()
    await writer.wait_closed()
    return int(start.split(" ")[1]), body


async def run(total):
    ready = asyncio.get_running_loop().create_future()
    server = asyncio.create_task(serve(ready, total))
    port = await ready
    queue = ["/page/0"]
    seen = {"/page/0"}
    head = 0
    fetched = not_found = html_bytes = nodes = links = external = 0
    lines = words_seen = title_bytes = checksum = 0
    while head < len(queue):
        path = queue[head]
        head += 1
        status, body = await fetch(port, path)
        fetched += 1
        if status != 200:
            not_found += 1
            continue
        html_bytes += len(body)
        page = browse(body)
        nodes += page.nodes
        title_bytes += page.title_bytes
        words_seen += page.layout.word_count
        for line in page.layout.lines:
            lines += 1
            for byte in line.encode():
                checksum = (checksum * 31 + byte) % 1000000007
            checksum = (checksum * 31 + 10) % 1000000007
        for href in page.layout.links:
            links += 1
            if href.startswith("/"):
                if href not in seen:
                    seen.add(href)
                    queue.append(href)
            else:
                external += 1
    await fetch(port, "/quit")
    served = await server
    print(
        f"mini_browser site={total} fetched={fetched} served={served} not_found={not_found} html_bytes={html_bytes} "
        f"nodes={nodes} links={links} external={external} lines={lines} words={words_seen} "
        f"title_bytes={title_bytes} checksum={checksum}"
    )


if __name__ == "__main__":
    asyncio.run(run(int(sys.argv[1])))
