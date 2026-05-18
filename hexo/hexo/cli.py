from __future__ import annotations

import argparse


def main() -> None:
    parser = argparse.ArgumentParser(prog="hexo")
    sub = parser.add_subparsers(dest="cmd")

    sub.add_parser("play", help="interactive REPL vs bot")
    selfplay = sub.add_parser("selfplay", help="bot vs bot")
    selfplay.add_argument("-n", type=int, default=1)
    sub.add_parser("bench", help="NPS benchmark")
    analyze = sub.add_parser("analyze", help="show eval + best line")
    analyze.add_argument("bsn")

    parser.parse_args()
    print("hexo cli stub")


if __name__ == "__main__":
    main()
