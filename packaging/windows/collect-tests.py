"""Copy the cross-compiled test executables listed by
`cargo test --no-run --message-format=json` (stdin) into <out-dir>.

Usage: cargo test ... --no-run --message-format=json | python3 collect-tests.py <out-dir>
"""

import json
import shutil
import sys
from pathlib import Path


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    count = 0
    for line in sys.stdin:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        executable = message.get("executable")
        if executable and message.get("profile", {}).get("test"):
            shutil.copy2(executable, out / Path(executable).name)
            count += 1
    if count == 0:
        sys.exit("no test executables found")
    print(f"{count} test executable(s) collected")


if __name__ == "__main__":
    main()
