# Tetris-By-PS3 (Tetris на Rust для PS3 Linux)

Terminal Tetris in Rust with zero dependencies (raw libc termios/poll), built and tested natively on PS3 ArchPOWER (powerpc64).

## About this project

This game was created entirely by an AI coding agent, from first commit to the
keyboard, rendering, and gameplay fixes. The human described what they wanted
and reported bugs; the agent wrote and debugged all the code.

- **Coding agent**: [Jcode](https://github.com/1jehuang/jcode), a tiny
  open-source terminal coding agent, running interactively in the PS3 Linux
  console.
- **LLM**: the agent was driven by GLM 5.3 (Zhipu AI) via its API.
- **Human role**: project idea, testing on real hardware, and bug reports
  (e.g. keyboard not responding, arrow keys inverted, pause screen flicker),
  which the agent diagnosed and fixed in a closed loop using automated
  pty-based tests.

## Running environment

- **Hardware**: Sony PlayStation 3 (Cell Broadband Engine, PowerPC 64-bit)
- **OS**: Arch Linux for PowerPC (ArchPOWER), PS3 Linux
- **Rust**: native powerpc64 toolchain (`cargo build --release` on the PS3
  itself, no cross-compilation)
- **Terminal**: any Linux console/terminal emulator with ANSI escape sequence
  support (tested on the PS3 console over SSH and locally)
- **Dependencies**: none. The game talks to the terminal directly through
  `termios` and `poll` syscalls via minimal `extern "C"` declarations, so it
  needs no crates.io packages and works in low-memory environments.

## Build & run
```
cd /home/c2j/workspace/rust-tetris
cargo build --release
./target/release/tetris
```

## Controls
- ←/→ or A/D: move
- ↑/W/Space: rotate
- ↓/S: soft drop
- C: hold piece
- P: pause
- Q: quit

Features: 7-bag randomizer, wall kicks, hold, next preview, score/lines/levels (speeds up), Russian UI.
