#!/usr/bin/env python3
# Deterministic protocol fixture only. NOT a neural network or playing engine.
import sys
import json

COLS = "ABCDEFGHJKLMNOPQRST"
for line in sys.stdin:
    q = json.loads(line)
    n = q["boardXSize"]
    occupied = {m[1] for m in q.get("initialStones", []) + q.get("moves", [])}
    root = f"{COLS[0]}{n}"
    balanced = f"{COLS[1]}{n}"
    reply = f"{COLS[2]}{n}"
    other = f"{COLS[3]}{n}"
    if not occupied:
        estimates = [(root, 0.91), (balanced, 0.49)]
        black_eval = 0.91
    elif balanced in occupied:
        estimates = [(reply, 0.51), (other, 0.89)]
        black_eval = 0.49
    else:
        estimates = [(reply, 0.86), (other, 0.92)]
        black_eval = 0.91
    policy = [0.0] * (n * n + 1)
    moves = []
    for i, (coord, value) in enumerate(estimates):
        if coord in occupied:
            continue
        x, y = COLS.index(coord[0]), n-int(coord[1:])
        policy[y*n+x] = 0.5
        moves.append({"move": coord, "winrate": value, "visits": 5,
                      "drawrate": 0.1, "prior": 0.5, "pv": [coord], "order": i})
    print(json.dumps({"id":q["id"], "rootInfo":{"visits":10, "winrate":black_eval},
                      "moveInfos":moves, "policy":policy, "forbidden":[],
                      "isDuringSearch":False}), flush=True)
