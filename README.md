# Tetris на Rust (PS3 Linux)

Terminal Tetris in Rust with zero dependencies (raw libc termios/poll), built and tested natively on PS3 ArchPOWER (powerpc64).

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
