#!/usr/bin/env python3
"""Fetch pinned official source and apply local, reproducible patches."""
import argparse
from pathlib import Path
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = "https://github.com/hzyhhzy/KataGomo.git"
PINS = {"Gom2024": "df152116e3787c75c6a3de099d261ca092b7dfc1",
        "Caro2024": "96e0bca2619566736b68251257d7999b33fa1dd9"}

def run(*args):
    subprocess.run([str(x) for x in args], check=True)

def replace(path, old, new):
    # Some upstream files contain legacy GBK comments. Keep every unrelated
    # byte and original newline style intact, instead of decoding as UTF-8.
    content = path.read_bytes()
    newline = b"\r\n" if b"\r\n" in content else b"\n"
    old = old.encode("ascii").replace(b"\n", newline)
    new = new.encode("ascii").replace(b"\n", newline)
    if new in content:
        return
    if content.count(old) != 1:
        raise RuntimeError(f"Unexpected source in {path}; refusing to patch")
    path.write_bytes(content.replace(old, new))

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--backend", choices=["EIGEN", "CUDA", "OPENCL", "TENSORRT"], default="EIGEN")
    p.add_argument("--build", action="store_true")
    p.add_argument("--branch", choices=list(PINS), action="append")
    p.add_argument("--cmake-arg", action="append", default=[])
    p.add_argument("--download-renju", action="store_true")
    p.add_argument("--jobs", type=int, default=2)
    args = p.parse_args()
    for branch in args.branch or PINS:
        target = ROOT / "engines" / branch
        target.parent.mkdir(exist_ok=True)
        if not target.exists():
            run("git", "clone", "--no-checkout", "--single-branch", "--branch", branch, UPSTREAM, target)
            run("git", "-C", target, "checkout", "--detach", PINS[branch])
        actual = subprocess.check_output(["git", "-C", str(target), "rev-parse", "HEAD"], text=True).strip()
        if actual != PINS[branch]:
            raise RuntimeError(f"{target}: expected {PINS[branch]}, got {actual}; refusing to overwrite")
        # Both upstream branches default to a compile-time maximum of 15.
        replace(target / "cpp/core/config.h", "#define COMPILE_MAX_BOARD_LEN 15", "#define COMPILE_MAX_BOARD_LEN 19")
        if branch == "Gom2024":
            # Report actual upstream Renju forbidden intersections, not guesses
            # based on policy scores. The array is present in patched results.
            replace(target / "cpp/command/analysis.cpp",
                    '    if(success)\n      pushToWrite(new string(ret.dump()));',
                    '''    if(success) {
      ret["forbidden"] = json::array();
      if(request->hist.rules.basicRule == Rules::BASICRULE_RENJU && request->nextPla == C_BLACK) {
        const Board& b = request->board;
        for(int y = 0; y < b.y_size; y++) for(int x = 0; x < b.x_size; x++) {
          Loc loc = Location::getLoc(x,y,b.x_size);
          if(b.colors[loc] == C_EMPTY && b.isForbidden(loc))
            ret["forbidden"].push_back(Location::toString(loc,b));
        }
      }
      pushToWrite(new string(ret.dump()));
    }''')
        if branch == "Caro2024":
            # Upstream discards updateRules' return value and passes JSON directly
            # as string, which breaks boolean/numeric rule values.
            replace(target / "cpp/game/rules.cpp",
                    "Rules::updateRules(key, iter.value(), rules);",
                    "string value = iter.value().is_string() ? iter.value().get<string>() : iter.value().dump();\n        rules = Rules::updateRules(key, value, rules);")
            replace(target / "cpp/game/rules.cpp",
                    'ret["sixwinrule"] = writeSixWinRule(sixWinRule);',
                    'ret["sixwinrule"] = writeSixWinRule(sixWinRule);\n  ret["wallblock"] = wallBlock;')
        if args.build:
            build = target / ("build-" + args.backend.lower())
            run("cmake", "-S", target / "cpp", "-B", build, "-DCMAKE_BUILD_TYPE=Release",
                "-DUSE_BACKEND=" + args.backend, "-DNO_GIT_REVISION=1", *args.cmake_arg)
            run("cmake", "--build", build, "--config", "Release", "--parallel", args.jobs)
    if args.download_renju:
        model = ROOT / "models" / "zhizi_renju28b_s1600.bin.gz"
        model.parent.mkdir(exist_ok=True)
        if not model.exists():
            tmp = model.with_suffix(".partial")
            urllib.request.urlretrieve(
                "https://github.com/hzyhhzy/KataGomo/releases/download/Gomoku_20250206/zhizi_renju28b_s1600.bin.gz", tmp)
            tmp.replace(model)
if __name__ == "__main__":
    main()
