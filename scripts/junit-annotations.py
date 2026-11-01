#!/usr/bin/env python3
"""Prints a GitHub Actions error annotation for each failed test in a JUnit file.

Annotations are readable through the API by anyone, unlike job logs.

    python3 scripts/junit-annotations.py services/gateway/target/nextest/ci/junit.xml
"""

import sys
import xml.etree.ElementTree as ET


def escape(text: str) -> str:
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def main() -> int:
    tree = ET.parse(sys.argv[1])
    failed = 0
    for case in tree.iter("testcase"):
        for problem in [*case.iter("failure"), *case.iter("error")]:
            failed += 1
            name = f"{case.get('classname', '')} {case.get('name', '')}".strip()
            # The message plus the tail of stdout/stderr, where panics and assertions are.
            output = "\n".join(
                (el.text or "") for el in [problem, *case.iter("system-out"), *case.iter("system-err")]
            )
            lines = [line for line in output.splitlines() if line.strip()][-40:]
            print(f"::error title={escape(name)}::{escape(chr(10).join(lines))}")
            break
    print(f"{failed} failed test(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
