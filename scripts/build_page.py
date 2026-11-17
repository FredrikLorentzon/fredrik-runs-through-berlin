"""Inject the compact run tracks into the HTML template to produce a single page."""

import argparse
from pathlib import Path

PLACEHOLDER = "__RUN_DATA__"


def main() -> None:
    """Write the template with the run data embedded in place of the placeholder."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("template", type=Path)
    parser.add_argument("data", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    template = args.template.read_text(encoding="utf-8")
    data = args.data.read_text(encoding="utf-8")
    if PLACEHOLDER not in template:
        raise SystemExit("template has no data placeholder")
    if "</" in data:
        raise SystemExit("data contains a closing tag sequence")
    args.output.write_text(template.replace(PLACEHOLDER, data, 1), encoding="utf-8")


if __name__ == "__main__":
    main()
