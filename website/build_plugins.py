"""Generate the Chinese README and website plugin lists from README.md."""
import argparse
import html
from pathlib import Path
import re
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent
START = "<!-- plugin-list:start -->"
END = "<!-- plugin-list:end -->"


def block(document):
    if document.count(START) != 1 or document.count(END) != 1:
        raise ValueError("Expected exactly one plugin-list marker pair")
    return document.split(START, 1)[1].split(END, 1)[0].strip()


def parse_plugins(document):
    lines = [line.strip() for line in block(document).splitlines() if line.strip()]
    if len(lines) < 3 or lines[:2] != ["| Plugin | Repository |", "| --- | --- |"]:
        raise ValueError("Plugin list needs its two-column header and at least one entry")
    plugins, seen = [], set()
    for line in lines[2:]:
        if not line.startswith("|") or not line.endswith("|"):
            raise ValueError("Use | Plugin name | HTTPS repository URL |")
        columns = [value.strip() for value in line[1:-1].split("|")]
        if len(columns) != 2 or not columns[0] or not columns[1]:
            raise ValueError("Every plugin needs only its name and repository URL")
        name, url = columns
        link = re.fullmatch(r"\[[^\]]+\]\((https://[^\s]+)\)", url)
        if link:
            url = link[1]
        parsed = urlsplit(url)
        if (parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password
                or any(character.isspace() or ord(character) < 32 for character in url)):
            raise ValueError("Plugin repositories must use HTTPS URLs without credentials")
        key = url.rstrip("/")
        if parsed.hostname == "github.com":
            key = key.casefold()
        if key in seen:
            raise ValueError("Duplicate plugin repository: " + url)
        seen.add(key)
        plugins.append((name, url))
    return plugins


def render_list(plugins):
    items = []
    for name, url in plugins:
        parsed = urlsplit(url)
        label = parsed.netloc + parsed.path.rstrip("/")
        items.append(
            '        <li><a href="' + html.escape(url, quote=True) + '" target="_blank" '
            'rel="noopener noreferrer"><strong class="plugin-name">' + html.escape(name)
            + '</strong><span class="plugin-repository">' + html.escape(label)
            + ' <span aria-hidden="true">↗</span></span></a></li>'
        )
    return '      <ul class="plugin-catalog">\n' + "\n".join(items) + '\n      </ul>'


def replace_block(document, content):
    block(document)
    before = document.split(START, 1)[0]
    after = document.split(END, 1)[1]
    return before + START + "\n" + content + "\n" + END + after


def generated_documents():
    source = (ROOT / "README.md").read_text(encoding="utf-8")
    plugins = parse_plugins(source)
    chinese_table = "| 插件 | 仓库 |\n| --- | --- |\n" + "\n".join(
        f"| {name} | {url} |" for name, url in plugins
    )
    return {
        ROOT / "README.zh-CN.md": replace_block(
            (ROOT / "README.zh-CN.md").read_text(encoding="utf-8"), chinese_table
        ),
        ROOT / "website/public/index.html": replace_block(
            (ROOT / "website/public/index.html").read_text(encoding="utf-8"), render_list(plugins)
        ),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Check generated lists without writing")
    args = parser.parse_args()
    for path, expected in generated_documents().items():
        if args.check:
            if path.read_text(encoding="utf-8") != expected:
                raise SystemExit("Plugin list is stale: " + str(path))
        else:
            path.write_text(expected, encoding="utf-8", newline="\n")
    print("PASS: plugin lists match README.md" if args.check else "Generated plugin lists from README.md")
