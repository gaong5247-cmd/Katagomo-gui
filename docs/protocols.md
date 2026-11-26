# Protocol compatibility (0.1.0)

Source references: [KataGomo GTP](https://github.com/hzyhhzy/KataGomo/blob/Gom2024/cpp/command/gtp.cpp), [KataGomo JSON analysis](https://github.com/hzyhhzy/KataGomo/blob/Gom2024/cpp/command/analysis.cpp), [Rapfi command implementation](https://github.com/dotb1ue/rapfi/blob/master/Rapfi/command/gomocup.cpp), [Gomocup specification](https://plastovicka.github.io/protocl2en.htm).

## Native
GTP and JSON analysis are direct pass-through modes. Caller supplies correct board/rules. The GUI uses JSON, not a fake UCI score. Winrate is expected result (win + 0.5 × draw), black perspective. Candidate drawrate is separate.

## Gomocup adapter
START 15/19; RECTSTART equal dimensions only; BEGIN; TURN x,y; BOARD lines x,y,1 (self) or x,y,2 (opponent), DONE; RESTART; TAKEBACK; INFO; ABOUT; END. BOARD infers the engine color from stone counts; no walls or pass support. STOP cancels the isolated process and emits best available candidate if present. No result before first report produces ERROR.

INFO RULE: 0 freestyle, 1 standard, 2/4 Renju. 5/6 Swap1/Swap2 are explicitly unsupported. TIMEOUT_TURN, TIMEOUT_MATCH, TIME_LEFT use milliseconds (bounded to 0.1–600 seconds; not a complete match clock). MAX_NODE maps to MCTS visits, not alpha-beta nodes. SHOW_DETAIL emits MESSAGE lines. GAME_TYPE/FOLDER informational only.

## Yixin extensions
YXBOARD, YXNBEST, YXSTOP, YXBLOCK, YXBLOCKUNDO, YXBLOCKRESET, YXSHOWINFO, YXHASHCLEAR (isolated searches already use fresh cache), YXSHOWFORBID (patched Gom2024 required; black Renju only).
YXNBEST emits the requested number of available candidates using MESSAGE and then best coordinate. This is an adapter output, not a byte-for-byte Rapfi diagnostic protocol. YXBLOCK and YXBLOCKUNDO take coordinates terminated by DONE. Exclusions are root-only.

## Rapfi extensions
LOADMODEL switches the active profile model path for subsequent searches, RELOADCONFIG reloads profile JSON and config is read on each process launch; TRACEBOARD prints the local position; BENCH measures real visits and wall time including initialization.

## Explicitly unsupported / not yet equivalent
SWAP2BOARD, YXBALANCEONE/TWO, YXSEARCHDEFEND, TRACESEARCH, EXPORTMODEL, hash dump/load/usage, all Rapfi database commands, depth limits, dynamic hash/threads/pondering/strength options, and full Rapfi/UCI-like output compatibility. They require separate implementations or have no faithful MCTS equivalent. Unknown commands return UNKNOWN; unsupported INFO returns ERROR, rather than silently faking support.

## Limitations
The native engine performs rule-aware search, but the GUI is a position editor and not yet a complete referee. Model/size compatibility, engine build and external GUI interoperability require real runtime verification. Caro requires its separate patched branch. Original 15×15 builds cannot analyze 19×19.
